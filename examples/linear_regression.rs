use halo2_base::halo2_proofs::halo2curves::bn256::Fr;
use halo2_base::utils::{ScalarField, BigPrimeField};
use halo2_base::AssignedValue;
use halo2_base::Context;
use zk_fixed_point_chip::gadget::linear_regression::LinearRegressionChip;
use zk_fixed_point_chip::nh_scaf::{NHCircuitInput, run_nh, nh_proove_verify};
#[allow(unused_imports)]
//use zk_fixed_point_chip::scaffold::{mock, prove};
use log::warn;
use std::cmp::min;
use std::env::{var, set_var};
use linfa::prelude::*;
use linfa_linear::LinearRegression;
use ndarray::{Array, Axis};
use zk_fixed_point_chip::scaffold::cmd::{Cli, SnarkCmd};
use clap::Parser;
use halo2_base::gates::circuit::builder::BaseCircuitBuilder;
use halo2_base::gates::circuit::CircuitBuilderStage;
use halo2_proofs::pasta::vesta::Base;
use std::time::{Duration, Instant};

pub fn train_native(
    train_x: Vec<Vec<f64>>, train_y: Vec<f64>, lr: f64, epoch: i32, batch_size: usize
) {
    let dim = train_x[0].len();
    let mut w = vec![0.; dim];
    let mut b = 0.;
    let n_batch = (train_x.len() as f64 / batch_size as f64).ceil() as i64;
    for idx_epoch in 0..epoch {
        println!("Epoch {:?}", idx_epoch + 1);
        for idx_batch in 0..n_batch {
            let batch_x = (&train_x[idx_batch as usize * batch_size..min(train_x.len(), (idx_batch as usize + 1) * batch_size)]).to_vec();
            let batch_y = (&train_y[idx_batch as usize * batch_size..min(train_y.len(), (idx_batch as usize + 1) * batch_size)]).to_vec();
            let n_sample = batch_x.len();
            let batch_lr = lr / n_sample as f64;

            let y_pred: Vec<f64> = batch_x.iter().map(|xi| {
                let mut yi = b;
                for j in 0..xi.len() {
                    yi += xi[j] * w[j];
                }

                yi
            }).collect();
            let diff_y: Vec<f64> = y_pred.iter().zip(batch_y).map(|(yi, ti)| yi - ti).collect();
            let loss: f64 = diff_y.iter().map(|x| x * x).sum::<f64>() / n_sample as f64 / 2.0;
            println!("loss: {:?}", loss);
            b = b - batch_lr * diff_y.iter().sum::<f64>();
            for j in 0..w.len() {
                w[j] = w[j] - batch_lr * diff_y.iter().zip(batch_x.iter()).map(|(diff_yi, batch_xi)| diff_yi * batch_xi[j]).sum::<f64>();
            }
            println!("w: {:?}, b: {:?}", w, b);
        }
    }
}

pub fn train<F: ScalarField>(
    //ctx: &mut Context<F>,
    bcb : &mut BaseCircuitBuilder<F>,
    input: (Vec<F>, F, Vec<Vec<f64>>, Vec<f64>, f64),
    make_public: &mut Vec<AssignedValue<F>>,
) where F: BigPrimeField {

    let lookup_bits =
        var("LOOKUP_BITS").unwrap_or_else(|_| panic!("LOOKUP_BITS not set")).parse().unwrap();

    //bcb.set_k(lookup_bits);
    let mut ctx = bcb.main(0);

    let chip = LinearRegressionChip::<F>::new(lookup_bits);

    let (w, b, train_x, train_y, learning_rate) = input;
    let mut w = w.iter().map(
        |wi| ctx.load_witness(*wi)).collect();
    let mut b = ctx.load_witness(b);
    let mut train_x_witness: Vec<Vec<AssignedValue<F>>> = vec![];
    for xi in train_x {
        train_x_witness.push(xi.iter().map(|xij| ctx.load_witness(chip.chip.quantization(*xij))).collect::<Vec<AssignedValue<F>>>());
    }
    let train_y: Vec<AssignedValue<F>> = train_y.iter().map(|yi| ctx.load_witness(chip.chip.quantization(*yi))).collect();

    (w, b) = chip.train_one_batch(ctx, w, b, train_x_witness, train_y, learning_rate);
    for wi in w {
        make_public.push(wi);
    }
    make_public.push(b);
    let param: Vec<f64> = make_public.iter().map(|x| chip.chip.dequantization(*x.value())).collect();
    println!("params: {:?}", param);
}


fn main() {
    set_var("RUST_LOG", "warn");
    env_logger::init();
    // genrally lookup_bits is degree - 1
    set_var("LOOKUP_BITS", 15.to_string());
    set_var("DEGREE", 16.to_string());

    let mut args_mock = Cli::parse();
    let mut cli_keygen = Cli::parse();

    let mut cli_verify =  Cli::parse();

    args_mock.command = SnarkCmd::Mock;
    cli_keygen.command = SnarkCmd::Keygen;
    cli_verify.command = SnarkCmd::Verify;

    let dataset = linfa_datasets::diabetes();
    let lin_reg = LinearRegression::new();
    let model = lin_reg.fit(&dataset).unwrap();
    println!("intercept:  {}", model.intercept());
    println!("parameters: {}", model.params());

    let start = Instant::now();

    let mut train_x: Vec<Vec<f64>> = vec![];
    let mut train_y: Vec<f64> = vec![];
    for (sample_x, sample_y) in dataset.sample_iter() {
        train_x.push(sample_x.iter().map(|xi| *xi).collect::<Vec<f64>>());
        train_y.push(*sample_y.iter().peekable().next().unwrap());
    }
    let dim = train_x[0].len();
    let mut w = vec![Fr::from(0); dim];
    let mut b = Fr::from(0);
    let epoch = 20;
    let learning_rate = 0.01;
    let batch_size: usize = 64;

    train_native(train_x.clone(), train_y.clone(), learning_rate, epoch, batch_size);

    // let mut builder: BaseCircuitBuilder<Fr> = BaseCircuitBuilder::from_stage(CircuitBuilderStage::Keygen);

    let n_batch = (train_x.len() as f64 / batch_size as f64).ceil() as i64;
    let dummy_inputs = (
        w.clone(),
        b.clone(),
        vec![vec![0.; dim]; batch_size as usize],
        vec![0.; batch_size as usize],
        0.01);

    let dd = NHCircuitInput{ data : dummy_inputs };
    run_nh(train, dd, cli_keygen);
    println!("KEY GEN DONE w: {:?}, b: {:?}", w, b);

    //let (pk, break_points) = gen_key(train, dummy_inputs);
    for idx_epoch in 0..epoch {

        println!("  ==> Will run epoch:{:?}", idx_epoch);
        warn!("Epoch {:?}", idx_epoch + 1);

        for idx_batch in 0..n_batch {
            let mut cli_proove = Cli::parse();
            cli_proove.command = SnarkCmd::Prove;
            if (idx_batch as usize + 1) * batch_size > train_x.len() {
                continue;
            }
            let batch_x = (&train_x[idx_batch as usize * batch_size..(idx_batch as usize + 1) * batch_size]).to_vec();
            let batch_y = (&train_y[idx_batch as usize * batch_size..min(train_y.len(), (idx_batch as usize + 1) * batch_size)]).to_vec();
            let private_inputs: (Vec<Fr>, Fr, Vec<Vec<f64>>, Vec<f64>, f64) = (w, b, batch_x, batch_y, learning_rate);

            let data_batch = NHCircuitInput{ data : private_inputs };

            let out = nh_proove_verify(train, cli_proove, data_batch);

            w = (&out[..dim]).iter().map(|wi| (*wi).clone()).collect();
            b = out[dim];
        }
    }
    println!("w: {:?}, b: {:?}", w, b);

    let duration = start.elapsed();
    println!("Time elapsed in [041] linear_regression::main0() is: {:?}", duration);

    // mock(train, (w, b, train_x, train_y));
    // prove(train, x0.clone(), x1.clone());
}
