# Cargo fmt
cargo fmt

# Basic builds
cargo build --bin onerom-lab
cargo build --release --bin onerom-lab

cargo build --no-default-features --features validate-28-fire --bin onerom-lab-fire --target thumbv8m.main-none-eabihf

# Builds of supported hardware variants including "repeat" support
cargo build --release --no-default-features --features f401re,repeat --bin onerom-lab
cargo build --release --no-default-features --features f405rg,repeat --bin onerom-lab
cargo build --release --no-default-features --features f411re,repeat --bin onerom-lab
cargo build --release --no-default-features --features f446re,repeat --bin onerom-lab

# Builds of supported hardware variants including "control" support
cargo build --release --no-default-features --features f401re,control --bin onerom-lab
cargo build --release --no-default-features --features f405rg,control --bin onerom-lab
cargo build --release --no-default-features --features f411re,control --bin onerom-lab
cargo build --release --no-default-features --features f446re,control --bin onerom-lab
# Builds of supported hardware variants including "oneshot" support
cargo build --release --no-default-features --features f401re,oneshot --bin onerom-lab
cargo build --release --no-default-features --features f405rg,oneshot --bin onerom-lab
cargo build --release --no-default-features --features f411re,oneshot --bin onerom-lab
cargo build --release --no-default-features --features f446re,oneshot --bin onerom-lab

# Check with logging
DEFMT_LOG=trace cargo build --bin onerom-lab
DEFMT_LOG=debug cargo build --release --bin onerom-lab
DEFMT_LOG=info cargo build --release --bin onerom-lab
DEFMT_LOG=warn cargo build --release --bin onerom-lab
DEFMT_LOG=error cargo build --release --bin onerom-lab

# Clippy
cargo clippy -- -D warnings --bin onerom-lab
cargo clippy --no-default-features --features f401re,repeat -- -D warnings --bin onerom-lab
cargo clippy --no-default-features --features f401re,control -- -D warnings --bin onerom-lab
cargo clippy --no-default-features --features f401re,oneshot -- -D warnings --bin onerom-lab

# Docs
cargo doc --bin onerom-lab --no-deps
