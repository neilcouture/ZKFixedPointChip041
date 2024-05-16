use halo2_base::utils::{ScalarField, BigPrimeField};
use halo2_base::AssignedValue;
use halo2_base::Context;
use zk_fixed_point_chip::gadget::fixed_point::{FixedPointChip, FixedPointInstructions};
use zk_fixed_point_chip::scaffold::cmd::{Cli, SnarkCmd};
#[allow(unused_imports)]

use zk_fixed_point_chip::scaffold::run;
use std::env::{var, set_var};
use clap::Parser;
use halo2_base::gates::circuit::builder::BaseCircuitBuilder;
use linfa_linear::LinearRegression;
use serde::{Deserialize, Serialize};


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CircuitInput {
    pub x: String, // field element, but easier to deserialize as a string
}

fn some_algorithm_in_zk<F: ScalarField>(
    builder: &mut BaseCircuitBuilder<F>,
    input: CircuitInput,
    make_public: &mut Vec<AssignedValue<F>>,
) where F: BigPrimeField
{
    //let x:f64 = f64::from_string(&input.x).unwrap(); //F::from_str_vartime(&input.x).expect("deserialize field element should not fail");

    let x:f64 = input.x.parse().unwrap(); //F::from_str_vartime(&input.x).expect("deserialize field element should not fail");

    let b2 = builder.clone();

    // `Context` can roughly be thought of as a single-threaded execution trace of a program we want to ZK prove. We do some post-processing on `Context` to optimally divide the execution trace into multiple columns in a PLONKish arithmetization
    // More advanced usage with multi-threaded witness generation is possible, but we do not explain it here
    let ctx = builder.main(0);

    // lookup bits must agree with the size of the lookup table, which is specified by an environmental variable
    let lookup_bits =
        var("LOOKUP_BITS").unwrap_or_else(|_| panic!("LOOKUP_BITS not set")).parse().unwrap();
    const PRECISION_BITS: u32 = 32;
    // fixed-point exp arithmetic
    let fixed_point_chip = FixedPointChip::<F, PRECISION_BITS>::default(lookup_bits, &b2);

    let x_decimal = x;
    let x = fixed_point_chip.quantization(x);
    println!("x: {:?}", x);

    // first we load a number `x` into as system, as a "witness"
    let x = ctx.load_witness(x);
    // by default, all numbers in the system are private
    // we can make it public like so:
    make_public.push(x);

    let exp_1 = fixed_point_chip.qexp2(ctx, x);
    let y_decimal = fixed_point_chip.dequantization(*exp_1.value());
    let y_native = x_decimal.exp2();
    println!(
        "###### zk-exp2({:.6}) = {}, native-exp({:.6}) = {:.6}, error = {:.6} ({:.6}%)",
        x_decimal, y_decimal, x_decimal, y_native,
        (y_decimal - y_native).abs(), (y_decimal - y_native).abs() / y_native.abs() * 100.0
    );
    make_public.push(exp_1);

    if x_decimal > 0f64 {
        let log_2 = fixed_point_chip.qlog2(ctx, x);
        let y_decimal_2 = fixed_point_chip.dequantization(*log_2.value());
        let y_native_2 = x_decimal.log2();
        println!(
            "###### zk-log2({:.6}) = {}, native-log2({:.6}) = {:.6}, error = {:.6} ({:.6}%)",
            x_decimal, y_decimal_2, x_decimal, y_native_2,
            (y_decimal_2 - y_native_2).abs(), (y_decimal_2 - y_native_2).abs() / y_native_2.abs() * 100.0
        );
        make_public.push(log_2);
    }

    let sin_x = fixed_point_chip.qsin(ctx, x);
    println!("sin_x_q: {:?}", *sin_x.value());
    let y_decimal_3 = fixed_point_chip.dequantization(*sin_x.value());
    let y_native_3 = x_decimal.sin();
    println!(
        "###### zk-sin({:.6}) = {}, native-sin({:.6}) = {:.6}, error = {:.6} ({:.6}%)",
        x_decimal, y_decimal_3, x_decimal, y_native_3,
        (y_decimal_3 - y_native_3).abs(), (y_decimal_3 - y_native_3).abs() / y_native_3.abs() * 100.0
    );
    make_public.push(sin_x);
}

fn main() {

    // let lin_reg = LinearRegression::new();

    env_logger::init();
    // genrally lookup_bits is degree - 1
    set_var("LOOKUP_BITS", 12.to_string());
    set_var("DEGREE", 13.to_string());

    let mut args_mock = Cli::parse();
    let mut args_keygen = Cli::parse();
    let mut args_proove = Cli::parse();
    let mut args_verify =  Cli::parse();

    println!("invoking mock...");
    args_mock.command = SnarkCmd::Mock;
    run(some_algorithm_in_zk, args_mock);

    println!("\ninvoking keygen...");
    args_keygen.command = SnarkCmd::Keygen;
    run(some_algorithm_in_zk, args_keygen);

    println!("\ninvoking prove...");
    args_proove.command = SnarkCmd::Prove;
    run(some_algorithm_in_zk, args_proove);

    println!("\ninvoking verify...");
    args_verify.command = SnarkCmd::Verify;
    run(some_algorithm_in_zk, args_verify);

    println!("\nDone!");
    // run mock prover
    // mock(some_algorithm_in_zk, -12.0);
    // mock(some_algorithm_in_zk, -1.88724767676867);
    // mock(some_algorithm_in_zk, 0.0);
    // mock(some_algorithm_in_zk, 1.0);
    // mock(some_algorithm_in_zk, 1.128);
    // mock(some_algorithm_in_zk, 2.0);
    // mock(some_algorithm_in_zk, 4.0);


}
