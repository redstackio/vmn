fn main() {
    if let Err(error) = vmn::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
