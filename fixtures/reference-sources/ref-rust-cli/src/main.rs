use std::io::{self, Read};

fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).expect("read stdin");
    let count = ref_rust_cli::word_count(&input);
    println!("{count}");
}
