fn main() {
    println!("cargo:rustflags=-Ctarget-feature=+aes,+ssse3");
}
