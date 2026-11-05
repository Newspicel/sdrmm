let name = variable::get("project-name");
let tag = variable::get("sdrmm_tag");

file::write("Cargo.toml", `[package]
name = "${name}"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
sdrmm-plugin = { git = "https://github.com/newspicel/sdrmm", tag = "${tag}" }
serde = { version = "1", features = ["derive"] }

[profile.release]
lto = true
codegen-units = 1
strip = true
`);

file::write("rust-toolchain.toml", `[toolchain]
channel = "stable"
targets = ["wasm32-unknown-unknown"]
`);
