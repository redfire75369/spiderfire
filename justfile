set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

default:
    @just --list

check *args:
  cargo check -F debugmozjs {{args}}

check-release *args:
  cargo check -r {{args}}

clippy *args:
  cargo clippy -F debugmozjs {{args}}

clippy-release *args:
  cargo clippy -r {{args}}

build *args:
  cargo build -F debugmozjs {{args}}

build-release *args:
  cargo build -r {{args}}

run *args:
  cargo run -F debugmozjs {{args}}

run-release *args:
  cargo run -r {{args}}

test *args:
  cargo nextest run -F debugmozjs --locked {{args}}

test-release *args:
  cargo nextest run  -r --locked {{args}}

docs:
  cargo doc --workspace --all-features --no-deps --document-private-items --keep-going

fmt *args:
  cargo +nightly fmt --all

lint:
  cargo +nightly fmt --check --all
  cargo clippy --workspace --all-targets -F debugmozjs --locked -- -D warnings

udeps:
  cargo +nightly udeps --workspace --all-targets --locked
