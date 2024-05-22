# ZKFixedPointChip041

To Run `examples/linear_regression.rs` :: `run  --package zk_fixed_point_chip  --example linear_regression -- --name lr -k 16 mock`
To Run `examples/fixed_point041` :: `run  --package zk_fixed_point_chip  --example fixed_point041 -- --name fp041 -k 16 mock`

ZK Fixed Point Arithmetic with its Application in Machine Learning based on [Halo2](https://github.com/privacy-scaling-explorations/halo2.git) & Axiom's [Halo2-base](https://github.com/axiom-crypto/halo2-lib).

## Features

* FixedPointChip: Fixed point arithmetic and math library
    * Support different kinds of precisions (from `32.32` to `63.63`) with automatically generated polynomial using Remez algorithm
    * Support negative number arithmetics with quantization
    * Support functions: `add`, `sub`, `mul`, `div`, `mod`, `sign`, `clip`, `polynomial`, `bit_xor`, `sum`, `neg`, `exp`, `log`, `pow`, `sqrt`, `max`, `sin`, `cos`, `tan`, `sinh`, `cosh`, etc.
* ZK-LR: LinearRegressionChip/LogisticRegressionChip
    * Support inference with vector multiplication and `sigmoid` (based on `exp`) using FixedPointChip
    * Support training with gradient descent algorithm
* ZK-DT: DecisionTreeChip
    * Support inference with tree traversal over the decision tree
    * Support training by building the decision tree recursively with the calculated Gini Impurity in each node


## Setup

Install rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Clone this repo:

```bash
git clone https://github.com/DCMMC/ZKFixedPointChip.git
cd ZKFixedPointChip
```

## Build & Run Examples

Fixed Point Arithmetic (exp2, log2, sin):

```bash
cargo run --example fixed_point
```

Linear Regression (Inference & Training):

```bash
cargo run --example linear_regression
```

Logistic Regression (Inference & Training):

```bash
cargo run --example logistic_regression
```

Decision Tree (Inference & Training):

```bash
cargo run --example decision_tree
```

> For visualizing, you should install `graphviz` and the generated `svg` file is located in `./figure/dt.svg`.

## Benchmark

![benchmark](./figure/benchmark.png)

> The complexity of decision tree training is proportional to $2^d$ where $d$ is the tree depth.

## Decision Tree Visualization

![dt](./figure/dt.svg)