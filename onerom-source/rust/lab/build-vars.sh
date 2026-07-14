set -e

cargo build --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f401re,qa,usb --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f405rg,qa,usb --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f411re,qa,usb --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f446re,qa,usb --target thumbv7em-none-eabi --release

cargo build --no-default-features --features f401re,control --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f405rg,control --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f411re,control --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f446re,control --target thumbv7em-none-eabi --release

cargo build --no-default-features --features f401re,oneshot --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f405rg,oneshot --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f411re,oneshot --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f446re,oneshot --target thumbv7em-none-eabi --release

cargo build --no-default-features --features f401re,repeat --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f405rg,repeat --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f411re,repeat --target thumbv7em-none-eabi --release
cargo build --no-default-features --features f446re,repeat --target thumbv7em-none-eabi --release
