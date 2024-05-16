use halo2_base::QuantumCell::{Constant, Existing, Witness};
use halo2_base::{
    gates::{GateChip, GateInstructions, RangeChip, RangeInstructions},
    utils::{biguint_to_fe, fe_to_biguint, BigPrimeField, ScalarField},
    AssignedValue, Context, QuantumCell,
};
use num_bigint::BigUint;
use num_integer::Integer;
use std::ops::Mul;
use std::{fmt::Debug, ops::Sub};
use halo2_base::gates::circuit::builder::BaseCircuitBuilder;

// #[derive(Clone, Copy, Debug, PartialEq)]
// pub enum FixedPointStrategy {
//     Vertical, // vanilla implementation with vertical basic gate(s)
// }

/// `PRECISION_BITS` indicates the precision of integer and fractional parts.
/// For example, `PRECISION_BITS = 32` indicates this chip implements 32.32 fixed point decimal arithmetics.
/// The valid range of the fixed point decimal is -max_value < x < max_value.
#[derive(Clone, Debug)]
pub struct FixedPointChip<F: BigPrimeField, const PRECISION_BITS: u32> {
    //strategy: FixedPointStrategy,
    pub gate: RangeChip<F>,
    pub quantization_scale: F,
    pub max_value: BigUint,
    pub bn254_max: F,
    pub negative_point: F,
    pub lookup_bits: usize,
    pub pow_of_two: Vec<F>,
}

impl<F: BigPrimeField, const PRECISION_BITS: u32> FixedPointChip<F, PRECISION_BITS> {
    pub fn new(lookup_bits: usize, bsb: &BaseCircuitBuilder<F>) -> Self {
        // Note 254/4 = 63.5
        assert!(PRECISION_BITS <= 63, "support only precision bits <= 63");
        assert!(PRECISION_BITS >= 32, "support only precision bits >= 32");

        // let ONE : F = F::ONE;

        let gate = bsb.range_chip();

        // Simple uniform symmetric quantization scheme which enforces zero point to be exactly 0
        // to reduce lots of computations.
        // Quantization: x_q = xS where S is `quantization_scale`
        // De-quantization: x = x_q / S
        let quantization_scale = F::from_u128(2u128.pow(PRECISION_BITS as u32));
        // Becuase BN254 is cyclic, negative number will be denoted as (-x) % m = m - x where m = 2^254,
        // in this chip, we treat all x > negative_point as a negative numbers.
        // P-1 for the field
        let bn254_max = biguint_to_fe(
            &BigUint::parse_bytes(&F::MODULUS[2..].bytes().collect::<Vec<u8>>(), 16)
                .unwrap()
                .sub(1u32),
        );
        // -max_value % m = negative_point
        // doesn't seem like the +1 is required here
        let negative_point = bn254_max - F::from_u128(2u128.pow(PRECISION_BITS * 2 + 1)) + F::ONE; //<F>::one();
        // min_value < x < max_value
        let max_value = BigUint::from(2u32).pow(PRECISION_BITS * 2);

        let mut pow_of_two = Vec::with_capacity(F::NUM_BITS as usize);
        let two = F::from(2);
        pow_of_two.push(F::ONE);
        pow_of_two.push(two);
        for _ in 2..F::NUM_BITS {
            pow_of_two.push(two * pow_of_two.last().unwrap());
        }

        Self {
            gate,
            quantization_scale,
            max_value,
            bn254_max,
            negative_point,
            lookup_bits,
            pow_of_two,
        }
    }

    pub fn default(lookup_bits: usize, bsb: &BaseCircuitBuilder<F>) -> Self {
        Self::new(lookup_bits, bsb)
    }


    fn nh_get_lower_128(&self, num : F) -> u128
    {
        let bytes = num.to_bytes_le();
        let mut lower_128 = 0u128;
        for (i, byte) in bytes.into_iter().enumerate().take(8) {
            lower_128 |= (byte as u128) << (i * 8);
        }
        lower_128
    }

    fn nh_from_bytes_wide(&self, bytes: &[u8; 64]) -> F
    {
        F::from_uniform_bytes(bytes)
    }


    pub fn quantization(&self, x: f64) -> F {
        let sign = x.signum();
        let x = x.abs();

        //let x_q = (x * self.quantization_scale.get_lower_128() as f64).round() as u128;

        let x_q = (x * self.nh_get_lower_128(self.quantization_scale) as f64).round() as u128;

        let x_q_biguint = BigUint::from(x_q).to_bytes_le();
        let mut x_q_bytes_le = [0u8; 64];
        for (idx, val) in x_q_biguint.iter().enumerate() {
            x_q_bytes_le[idx] = *val;
        }
        let mut x_q_f = self.nh_from_bytes_wide(&x_q_bytes_le);
        if sign < 0.0 {
            x_q_f = self.bn254_max - x_q_f + F::ONE;
        }

        x_q_f
    }


    pub fn dequantization(&self, x: F) -> f64 {
        let mut x_mut = x;
        let negative = if x > self.negative_point {
            x_mut = self.bn254_max - x - F::ONE; // Shouldn't this be +F::one()?
            -1f64
        } else {
            1f64
        };
        //let x_u128: u128 = x_mut.get_lower_128();
        let x_u128: u128 = self.nh_get_lower_128(x_mut);
        //let quantization_scale = self.quantization_scale.get_lower_128();

        let quantization_scale = self.nh_get_lower_128(self.quantization_scale);

        let x_int = (x_u128 / quantization_scale) as f64;
        let x_frac = (x_u128 % quantization_scale) as f64 / quantization_scale as f64;
        let x_deq = negative * (x_int + x_frac);

        x_deq
    }

    fn generate_exp2_poly(&self) -> Vec<QuantumCell<F>> {
        // generated by remez algorithm, poly degree 12, precision bits: 64.28
        let coef: Vec<F> = [
            3.6240421303547230336183979205877e-11,
            4.1284327467833130245549169910389e-10,
            0.0000000071086385644026346316624185550542,
            0.00000010172297085296590958930245291448,
            0.0000013215904023658396206789543841996,
            0.000015252713316417140696221389106544,
            0.00015403531076657894204857389177279,
            0.0013333558131297097698435464957392,
            0.0096181291078409107025643582456283,
            0.055504108664804181586140094858174,
            0.24022650695910142332414229540187,
            0.69314718055994529934452147700678,
            1.0,
        ]
        .into_iter()
        .map(|c| self.quantization(c))
        .collect();

        coef.iter().map(|x| Constant(*x)).collect()
    }

    fn generate_log_poly(&self) -> Vec<QuantumCell<F>> {
        // generated by lolremez -d 14  -r "2:4" "log2(x)"
        // Estimated max error: 6.4897885416380772e-13
        let coef: Vec<F> = [
            -3.319586265362338e-08,
            1.4957235315170112e-06,
            -3.1350053389526744e-05,
            0.00040554177582512901,
            -0.0036218342998850703,
            0.023663846121538389,
            -0.11691877183255484,
            0.44524062371564499,
            -1.3195777548208449,
            3.0518128028712077,
            -5.4904626000399528,
            7.6298580090181591,
            -8.1653313719804235,
            7.1389971101896279,
            -3.1937385492842112,
        ]
        .into_iter()
        .map(|c| self.quantization(c))
        .collect();

        coef.iter().map(|x| Constant(*x)).collect()
    }

    fn generate_sin_poly(&self) -> Vec<QuantumCell<F>> {
        // generated by lolremez -d 14  -r "0:pi" "sin(x)"
        // Estimated max error: 1.9323057584419826e-15
        let coef: Vec<F> = [
            -1.1008071636607462e-11,
            2.4208013888629323e-10,
            -3.8584805817996712e-10,
            -2.3786993104309845e-08,
            -2.9795813710683115e-09,
            2.7608543130047009e-06,
            -6.4467066994122565e-09,
            -0.00019840680551418068,
            -3.839555844512214e-09,
            0.0083333350601673614,
            -5.0943769725466814e-10,
            -0.16666666657583049,
            -8.5029878414113731e-12,
            1.0000000000003146,
            -1.9323057584419828e-15,
        ]
        .into_iter()
        .map(|c| self.quantization(c))
        .collect();

        coef.iter().map(|x| Constant(*x)).collect()
    }
}

pub trait FixedPointInstructions<F: ScalarField, const PRECISION_BITS: u32> {
    /// Fixed point decimal and its arithmetic functions.
    /// [ref] https://github.com/XMunkki/FixPointCS/blob/c701f57c3cfe6478d1f6fd7578ae040c59386b3d/Cpp/Fixed64.h
    /// [ref] https://github.com/abdk-consulting/abdk-libraries-solidity/blob/master/ABDKMath64x64.sol
    ///
    type Gate: GateInstructions<F>;
    type RangeGate: RangeInstructions<F>;

    /// returns the GateChip associated with the FixedPointChip
    fn gate(&self) -> &Self::Gate;
    /// returns the RangeGateChip associated with the FixedPointChip
    fn range_gate(&self) -> &Self::RangeGate;

    fn qabs(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn is_neg(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn sign(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn cond_neg(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        is_neg: AssignedValue<F>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField;

    /// clip the value to ensure it's in the valid range: (-2^p, 2^p), i.e., simulate overflow
    /// Warning: assumes a < 2^{p+1},This may fail silently if a is too large
    /// (e.g., mul of two large number leads to 2^{2p}).
    fn clip(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn polynomial<QA>(
        &self,
        ctx: &mut Context<F>,
        x: impl Into<QuantumCell<F>>,
        coef: impl IntoIterator<Item = QA>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
        QA: Into<QuantumCell<F>> + Debug + Copy;

    fn bit_xor(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = self.gate().add(ctx, Constant(F::ZERO), a.into());
        let b = self.gate().add(ctx, Constant(F::ZERO), b.into());
        self.gate().assert_bit(ctx, a);
        self.gate().assert_bit(ctx, b);
        let ab = self.gate().add(ctx, a, b);
        let one = self.gate().add(ctx, Constant(F::ONE), Constant(F::ZERO));
        let xor = self.gate().is_equal(ctx, ab, one);

        xor
    }

    fn qsum<Q>(&self, ctx: &mut Context<F>, a: impl IntoIterator<Item = Q>) -> AssignedValue<F>
    where
        Q: Into<QuantumCell<F>>,
    {
        self.gate().sum(ctx, a)
    }

    fn neg(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        self.gate().neg(ctx, a)
    }

    fn qadd(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn qsub(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn _qmul_unscaled(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn qmul(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn qdiv(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn inner_product<QA>(
        &self,
        ctx: &mut Context<F>,
        a: impl IntoIterator<Item = QA>,
        b: impl IntoIterator<Item = QA>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
        QA: Into<QuantumCell<F>> + Copy;

    fn qmod(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField;

    /// exp2
    fn qexp2(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    /// log
    fn qlog2(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    /// sin
    fn qsin(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn qcos(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn check_power_of_two(
        &self,
        ctx: &mut Context<F>,
        pow2_exponent: AssignedValue<F>,
        exponent: AssignedValue<F>,
    ) where
        F: BigPrimeField;

    fn qtan(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let sin_a = self.qsin(ctx, a);
        let cos_a = self.qcos(ctx, a);
        let y = self.qdiv(ctx, sin_a, cos_a);

        y
    }

    fn qexp(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn qsinh(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn qcosh(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn qtanh(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let sinh = self.qsinh(ctx, a);
        let cosh = self.qcosh(ctx, a);
        let y = self.qdiv(ctx, sinh, cosh);

        y
    }

    fn qmax(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn qmin(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn qlog(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn qpow(
        &self,
        ctx: &mut Context<F>,
        x: impl Into<QuantumCell<F>>,
        exponent: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        // x^a = exp(a * log(x))
        let logx = self.qlog(ctx, x);
        let alogx = self.qmul(ctx, exponent, logx);
        let y = self.qexp(ctx, alogx);

        y
    }

    fn qsqrt(&self, ctx: &mut Context<F>, x: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField;

    fn signed_div_scale(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
    ) -> (AssignedValue<F>, AssignedValue<F>);
}

impl<F: BigPrimeField, const PRECISION_BITS: u32> FixedPointInstructions<F, PRECISION_BITS>
    for FixedPointChip<F, PRECISION_BITS>
{
    type Gate = GateChip<F>;
    type RangeGate = RangeChip<F>;

    fn range_gate(&self) -> &Self::RangeGate {
        &self.gate
    }

    fn gate(&self) -> &Self::Gate {
        &self.gate.gate()
    }


    /// Adds the given numbers and returns their sum
    /// Simply uses the GateChip fn add
    /// is unsafe, value can overflow
    fn qadd(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        self.gate().add(ctx, a, b)
    }

    /// Subtracts the given numbers and returns the result
    /// Simply uses the GateChip fn sub
    fn qsub(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        self.gate().sub(ctx, a, b)
    }

    // Calls is_neg; Is very expensive
    fn qabs(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let a_reverse = self.gate().neg(ctx, a);
        let is_neg = self.is_neg(ctx, a);
        let a_abs = self.gate().select(ctx, a_reverse, a, is_neg);

        a_abs
    }

    // Is very expensive
    fn is_neg(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let a_num_bits = 254;
        let (a_shift, _) = self.range_gate().div_mod(
            ctx,
            a,
            BigUint::from(2u32).pow((PRECISION_BITS * 2 + 1) as u32),
            a_num_bits,
        );
        let is_pos = self.gate().is_zero(ctx, a_shift);
        let is_neg = self.gate().not(ctx, is_pos);

        is_neg
    }

    // Calls is_neg; Is very expensive
    fn cond_neg(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        is_neg: AssignedValue<F>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let neg_a = self.gate().neg(ctx, a);
        // self.gate().assert_bit(ctx, is_neg_assigned);
        let res = self.gate().select(ctx, neg_a, a, is_neg);

        res
    }

    // Calls is_neg; Is very expensive
    fn sign(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let pos_one = Constant(F::ONE);
        // (-1) % m where m = 2^254
        let neg_one = self.gate().neg(ctx, pos_one);
        let is_neg = self.is_neg(ctx, a);
        let res = self.gate().select(ctx, neg_one, pos_one, is_neg);

        res
    }

    // Calls is_neg; Is very expensive
    // calls qabs which also calls is_neg- can be removed
    fn clip(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let sign = self.is_neg(ctx, a);
        let a_abs = self.qabs(ctx, a);
        let a_num_bits = 254;
        let m = self.max_value.clone();
        // clipped = a % m
        // TODO (Wentao XIAO) should we just throw panic when overflow?
        let (_, unsigned_cliped) = self.range_gate().div_mod(ctx, a_abs, m, a_num_bits);
        let clipped = self.cond_neg(ctx, unsigned_cliped, sign);

        clipped
    }

    /// for quantised [a] =x*S and [b] =y*S (where S is the quantisation factor = 2^P) this function returns (x*y)S^2
    /// is unsafe in general
    fn _qmul_unscaled(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let b = b.into();

        let ab = self.gate().mul(ctx, a, b);
        return ab;
    }

    // is unsafe in general
    fn qmul(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let b = b.into();

        let ab = self.gate().mul(ctx, a, b);
        // for quantised a and b - call them a_q= a.S, b_q= b.S
        // "quantised" ab above is = (ab).S^2
        // need to divide this by S to get the correct result
        // This step can be batched for multiple multiplications- like inner product
        let (res, _) = self.signed_div_scale(ctx, ab);

        res
    }

    fn qmod(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        // b must be positive
        let a = a.into();
        let b = b.into();
        let a_sign = self.is_neg(ctx, a);
        let b_sign = self.is_neg(ctx, b);
        self.gate().assert_is_const(ctx, &b_sign, &F::ZERO);
        let a_abs = self.qabs(ctx, a);
        let a_num_bits = PRECISION_BITS as usize * 4;
        let b_num_bits = PRECISION_BITS as usize * 2;
        let (_, res_abs) = self.range_gate().div_mod_var(ctx, a_abs, b, a_num_bits, b_num_bits);
        let res_abs_comp = self.gate().sub(ctx, b, res_abs);
        let res = self.gate().select(ctx, res_abs_comp, res_abs, a_sign);

        res
    }

    fn qdiv(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let b = b.into();
        let a_sign = self.is_neg(ctx, a);
        let b_sign = self.is_neg(ctx, b);
        let a_abs = self.qabs(ctx, a);
        let b_abs = self.qabs(ctx, b);
        // Because a_rescale \in [0, 2^{4p}) and b \in [0, 2^p)
        let a_num_bits = PRECISION_BITS as usize * 4;
        let b_num_bits = PRECISION_BITS as usize * 2;
        let a_rescale = self.gate().mul(ctx, a_abs, Constant(self.quantization_scale));
        let (res_abs, _) =
            self.range_gate().div_mod_var(ctx, a_rescale, b_abs, a_num_bits, b_num_bits);
        let ab_sign = self.bit_xor(ctx, a_sign, b_sign);
        let res = self.cond_neg(ctx, res_abs, ab_sign);

        res
    }

    fn polynomial<QA>(
        &self,
        ctx: &mut Context<F>,
        x: impl Into<QuantumCell<F>>,
        coef: impl IntoIterator<Item = QA>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
        QA: Into<QuantumCell<F>> + Debug + Copy,
    {
        let x = x.into();
        let mut intermediates = vec![Constant(F::ZERO)];
        let coef_iter: Vec<QA> = coef.into_iter().collect();
        let last_idx_coef = coef_iter.len() - 1;
        let mut result: AssignedValue<F> = self.qadd(ctx, x, Constant(F::ZERO));
        for (idx, c) in coef_iter.into_iter().enumerate() {
            let last_y = *intermediates.get(intermediates.len() - 1).unwrap();
            let y_add = self.qadd(ctx, last_y, c);
            intermediates.push(Existing(y_add));
            if idx < last_idx_coef {
                let y = self.qmul(ctx, x, Existing(y_add));
                intermediates.push(Existing(y));
            } else {
                result = y_add;
            }
        }

        result
    }

    fn check_power_of_two(
        &self,
        ctx: &mut Context<F>,
        pow2_exponent: AssignedValue<F>,
        exponent: AssignedValue<F>,
    ) where
        F: BigPrimeField,
    {
        let range_bits = PRECISION_BITS as usize * 2;
        let bits = self.gate().num_to_bits(ctx, pow2_exponent, range_bits);
        let sum_of_bits = self.gate().sum(ctx, bits.clone());
        let sum_of_bits_m1 = self.gate().sub(ctx, sum_of_bits, Constant(F::ONE));
        let is_zero = self.gate().is_zero(ctx, sum_of_bits_m1);
        // ensure the bits of pow2_exponent has only one of bit one.
        self.gate().assert_is_const(ctx, &is_zero, &F::ONE);
        let bit = self.gate().select_from_idx(ctx, bits.into_iter().map(|x| Existing(x)), exponent);
        let bit_m1 = self.gate().sub(ctx, bit, Constant(F::ONE));
        let is_zero_bit_m1 = self.gate().is_zero(ctx, bit_m1);
        // ensures bits[expnent] is exact bit one
        self.gate().assert_is_const(ctx, &is_zero_bit_m1, &F::ONE);
    }

    fn qexp2(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let a_abs = self.qabs(ctx, a);
        let num_bits = PRECISION_BITS as usize * 2;
        let shift = 2u128.pow(PRECISION_BITS);
        let (int_part, frac_part) =
            self.range_gate().div_mod(ctx, Existing(a_abs), shift, num_bits);
        // int_part must be small as large number leads to overflow.
        let pow_of_two: Vec<QuantumCell<F>> =
            self.pow_of_two.iter().map(|x| Constant(*x)).collect();
        let int_part_pow2 = self.gate().select_from_idx(ctx, pow_of_two, int_part);
        let coef = self.generate_exp2_poly();
        let y_frac = self.polynomial(ctx, frac_part, coef);
        let res_pos = self.gate().mul(ctx, Existing(int_part_pow2), Existing(y_frac));

        let one = Constant(F::from_u128(shift));
        let res_neg = self.qdiv(ctx, one, res_pos);
        let is_neg = self.is_neg(ctx, a);
        let res = self.gate().select(ctx, res_neg, res_pos, is_neg);

        res
    }

    fn qlog2(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let a_assigned = self.gate().add(ctx, a, Constant(F::ZERO));
        let is_neg = self.is_neg(ctx, a);
        let is_zero = self.gate().is_zero(ctx, a_assigned);
        let is_invalid = self.gate().or(ctx, is_neg, is_zero);
        self.gate().assert_is_const(ctx, &is_invalid, &F::ZERO);
        let num_bits = (PRECISION_BITS * 2) as usize;
        let num_digits = a_assigned
            .value()
            .to_repr()
            .as_ref()
            .iter()
            .flat_map(|byte| (0..8u32).map(|i| (*byte as u64 >> i) & 1))
            .enumerate()
            .fold(1u64, |acc, (idx, val)| if val == 1u64 { idx as u64 } else { acc });
        let pow1 = self.gate().pow_of_two()[num_digits as usize];
        let pow1_witness = self.gate().add(ctx, Witness(pow1), Constant(F::ZERO));
        let exp1 = self.gate().add(ctx, Witness(F::from(num_digits)), Constant(F::ZERO));
        self.check_power_of_two(ctx, pow1_witness, exp1);
        let pow2_witness = self.gate().mul(ctx, pow1_witness, Constant(F::from(2)));
        let exp2 = self.gate().add(ctx, exp1, Constant(F::ONE));
        self.check_power_of_two(ctx, pow2_witness, exp2);
        // pow1 <= a < pow2, pow1 = 2^n, pow2 = 2^{n+1}
        let a_lt_pow2 = self.range_gate().is_less_than(ctx, a, pow2_witness, num_bits);
        let a_gt_pow1 = self.range_gate().is_less_than(ctx, pow1_witness, a, num_bits);
        let a_eq_pow1 = self.gate().is_equal(ctx, a, pow1_witness);
        let a_ge_pow1 = self.gate().or(ctx, a_eq_pow1, a_gt_pow1);
        let a_bound = self.gate().and(ctx, a_lt_pow2, a_ge_pow1);
        self.gate().assert_is_const(ctx, &a_bound, &F::ONE);

        // shift a to ensure a = 2^m * k, m \in Z, 2^{1} <= k < 2^{2}
        let shift = self.gate().sub(ctx, Constant(F::from(PRECISION_BITS as u64 + 2)), exp2);
        let is_shift_neg = self.is_neg(ctx, shift);
        let shift_abs = self.qabs(ctx, shift);
        let shift_pow2 = self.gate().pow_of_two()[shift_abs.value().get_lower_32() as usize];
        let shift_pow2_witness = self.gate().add(ctx, Witness(shift_pow2), Constant(F::ZERO));
        self.check_power_of_two(ctx, shift_pow2_witness, shift_abs);
        let a_ls = self.gate().mul(ctx, a, shift_pow2_witness);
        let (a_rs, _) = self.range_gate().div_mod_var(
            ctx,
            a,
            shift_pow2_witness,
            num_bits,
            PRECISION_BITS as usize + 1,
        );
        let a_norm = self.gate().select(ctx, a_rs, a_ls, is_shift_neg);

        let coef = self.generate_log_poly();
        let log_a_norm = self.polynomial(ctx, a_norm, coef);

        let log_shift = self.gate().neg(ctx, shift);
        let log_shift_q = self.gate().mul(ctx, log_shift, Constant(self.quantization_scale));
        let res = self.gate().add(ctx, log_a_norm, log_shift_q);

        res
    }

    fn bit_xor(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = self.gate().add(ctx, Constant(F::ZERO), a.into());
        let b = self.gate().add(ctx, Constant(F::ZERO), b.into());
        self.gate().assert_bit(ctx, a);
        self.gate().assert_bit(ctx, b);
        let ab = self.gate().add(ctx, a, b);
        let one = self.gate().add(ctx, Constant(F::ONE), Constant(F::ZERO));
        let xor = self.gate().is_equal(ctx, ab, one);

        xor
    }

    fn qsin(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let a_abs = self.qabs(ctx, a);
        let a_sign = self.is_neg(ctx, a);
        let pi_2 = Constant(self.quantization(std::f64::consts::PI * 2.0));
        // |a| % 2pi
        let a_mod = self.qmod(ctx, a_abs, pi_2);
        let pi = Constant(self.quantization(std::f64::consts::PI));
        // (|a| % 2pi) - pi
        let a_mpi = self.qsub(ctx, a_mod, pi);
        let is_neg_a_mpi = self.is_neg(ctx, a_mpi);
        let coef1 = self.generate_sin_poly();
        let sin_a_mod = self.polynomial(ctx, a_mod, coef1);
        let coef2 = self.generate_sin_poly();
        // -sin(a-pi) for pi <= a < 2pi
        let sin_a_mpi_rev = self.polynomial(ctx, a_mpi, coef2);
        let sin_a_mpi = self.neg(ctx, sin_a_mpi_rev);
        let sin_a_abs = self.gate().select(ctx, sin_a_mod, sin_a_mpi, is_neg_a_mpi);
        let sin_a = self.cond_neg(ctx, sin_a_abs, a_sign);

        sin_a
    }

    fn qcos(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let half_pi = ctx.load_constant(self.quantization(std::f64::consts::FRAC_PI_2));
        let a_plus_half_pi = self.qadd(ctx, a, half_pi);
        let y = self.qsin(ctx, a_plus_half_pi);

        y
    }

    fn inner_product<QA>(
        &self,
        ctx: &mut Context<F>,
        a: impl IntoIterator<Item = QA>,
        b: impl IntoIterator<Item = QA>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
        QA: Into<QuantumCell<F>> + Copy,
    {
        // you can make this abou 10% more efficient by using the builtin inner-product function
        let a: Vec<QA> = a.into_iter().collect();
        let b: Vec<QA> = b.into_iter().collect();
        assert!(a.len() == b.len());
        // using the fact that FixedPointChip's 0 is at F::zero()
        let mut res_s = ctx.load_witness(F::ZERO);
        self.gate().assert_is_const(ctx, &res_s, &F::ZERO);

        for (ai, bi) in a.iter().zip(b.iter()).into_iter() {
            res_s = self.gate().mul_add(ctx, *ai, *bi, res_s);
        }
        // Implementing this way allows us to amortize the cost of calling this expensive rescaling- will also lead to more accuracy
        let (res, _) = self.signed_div_scale(ctx, res_s);
        res
    }

    fn qexp(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        // e^x == 2^(x / ln(2))
        let ln2 = ctx.load_constant(self.quantization(2.0f64.ln()));
        let x1 = self.qdiv(ctx, a, ln2);
        let y = self.qexp2(ctx, x1);

        y
    }

    fn qsinh(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let ea = self.qexp(ctx, a);
        let na = self.neg(ctx, a);
        let ena = self.qexp(ctx, na);
        let nume = self.qsub(ctx, ea, ena);
        let two = ctx.load_constant(self.quantization(2.0));
        let y = self.qdiv(ctx, nume, two);

        y
    }

    fn qcosh(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let ea = self.qexp(ctx, a);
        let na = self.neg(ctx, a);
        let ena = self.qexp(ctx, na);
        let nume = self.qadd(ctx, ea, ena);
        let two = ctx.load_constant(self.quantization(2.0));
        let y = self.qdiv(ctx, nume, two);

        y
    }

    fn qmax(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let b = b.into();
        let amb = self.qsub(ctx, a, b);
        let sign_amb = self.is_neg(ctx, amb);
        let y = self.gate().select(ctx, b, a, sign_amb);

        y
    }

    fn qmin(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
        b: impl Into<QuantumCell<F>>,
    ) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let a = a.into();
        let b = b.into();
        let amb = self.qsub(ctx, a, b);
        let sign_amb = self.is_neg(ctx, amb);
        let y = self.gate().select(ctx, a, b, sign_amb);

        y
    }

    fn qlog(&self, ctx: &mut Context<F>, a: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        // log(x) = log2(x) / log2(e)
        let log2e = ctx.load_constant(self.quantization(std::f64::consts::LOG2_E));
        let log2a = self.qlog2(ctx, a);
        let y = self.qdiv(ctx, log2a, log2e);

        y
    }

    // Could be better to just verify y*y = x
    fn qsqrt(&self, ctx: &mut Context<F>, x: impl Into<QuantumCell<F>>) -> AssignedValue<F>
    where
        F: BigPrimeField,
    {
        let half = ctx.load_constant(self.quantization(0.5));
        self.qpow(ctx, x, half)
    }

    /// This shifts things by 2^precision to the right- useful after multiplying two things
    fn signed_div_scale(
        &self,
        ctx: &mut Context<F>,
        a: impl Into<QuantumCell<F>>,
    ) -> (AssignedValue<F>, AssignedValue<F>) {
        // a = b * q + r, r in [0, b), q in [-2^n, 2^n]
        let a = a.into();
        // b = 2^p
        let b = fe_to_biguint(&self.quantization_scale);
        // 252 = 63 * 4
        // the raw result of multiplying two positive or negative numbers is at most 252 bit
        // similarly the raw result of multiplying two oppositely signed numbers is p - (something 252 bit)
        // p is at least 254 bit- I think
        // 2^254-2^252 > 2^252
        // is_neg only on BigInt - so not costly yet
        // because of the above reasoning this correctly calculates whether a is supposed to be negative or not
        let a_is_neg = fe_to_biguint(a.value()) > BigUint::from(2u32).pow(252u32);
        let (q, r) = if a_is_neg {
            let a_abs = fe_to_biguint(&(self.bn254_max - a.value() + F::ONE));
            let q = fe_to_biguint(&self.bn254_max) - a_abs.div_ceil(&b) + BigUint::from(1u32);
            let r = fe_to_biguint::<F>(a.value())
                - fe_to_biguint::<F>(
                    &(biguint_to_fe::<F>(&b.clone()) * biguint_to_fe::<F>(&q.clone())),
                );
            // assert!(*a.value() == biguint_to_fe::<F>(&b) * biguint_to_fe::<F>(&q) + biguint_to_fe::<F>(&r));
            (q, r)
        } else {
            fe_to_biguint(a.value()).div_mod_floor(&b)
        };
        // up to this point no constraints have been added; so none of this costs anything
        ctx.assign_region(
            [
                Witness(biguint_to_fe(&r)),
                Constant(biguint_to_fe(&b)),
                Witness(biguint_to_fe(&q)),
                a,
            ],
            [0],
        );
        let rem = ctx.get(-4);
        let div = ctx.get(-2);

        // lookup constraint
        self.range_gate().check_big_less_than_safe(ctx, rem, b);
        // a < 2^{4p}, b = 2^p, so |q| < 2^{3p}
        // abs_bound = 2^{3P}
        let abs_bound = BigUint::from(2u32).pow(PRECISION_BITS * 3 as u32);
        // abs_bound_minus1 = 2^{3P} - 1
        let abs_bound_minus1 = abs_bound.clone() - BigUint::from(1u32);
        // new_bound = 2*2^{3P} - 1
        let new_bound = abs_bound.clone() * BigUint::from(2u32) - BigUint::from(1u32);
        // div_plus_abs_bound_minus1 = div + 2^{3P} - 1
        let div_plus_abs_bound_minus1 =
            self.gate().add(ctx, div, Constant(biguint_to_fe(&abs_bound_minus1)));

        // |q| < 2^{3p} <==> div_plus_abs_bound_minus1 in [0, 2*2^{3P} - 1)
        // lookup constraint
        self.range_gate().check_big_less_than_safe(ctx, div_plus_abs_bound_minus1, new_bound);

        (div, rem)
    }
}
