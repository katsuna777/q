# Source this file to use a project-local Rust toolchain:  source ./dev-env.sh
# Optional: OTPUSKA_TOOLCHAIN=/path/with/cargo-and-rustup (contains cargo/bin and rustup/).
# Without it the Rust from your PATH is used.
if [ -n "${OTPUSKA_TOOLCHAIN:-}" ] && [ -d "$OTPUSKA_TOOLCHAIN/cargo/bin" ]; then
  export RUSTUP_HOME="$OTPUSKA_TOOLCHAIN/rustup"
  export CARGO_HOME="$OTPUSKA_TOOLCHAIN/cargo"
  export PATH="$CARGO_HOME/bin:$PATH"
elif [ -d "$HOME/my/devbox/.toolchain/cargo/bin" ]; then
  # The toolchain used while developing this project on its author's machine.
  export RUSTUP_HOME="$HOME/my/devbox/.toolchain/rustup"
  export CARGO_HOME="$HOME/my/devbox/.toolchain/cargo"
  export PATH="$CARGO_HOME/bin:$PATH"
fi
# Gentle on the machine: few parallel jobs by default.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-3}"
