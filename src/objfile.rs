use std::collections::BTreeMap;
use std::io::BufReader;
use std::io::BufRead;
use std::process::Command;

pub fn dump_file(file_path: &str)
    -> (BTreeMap<String, String>, BTreeMap<String, String>)
{
    let stdout = Command::new("sh")
        .arg("-c")
        .arg(format!("nm {}", file_path))
        .output()
        .expect("Should call nm")
        .stdout;
    let reader = BufReader::new(stdout.as_slice());

    let mut text_symbols = BTreeMap::new();
    let mut weak_symbols = BTreeMap::new();
    for line in reader.lines() {
        let line = line.unwrap();
        let mut cols = line.split(" ");
        let addr = cols.next().expect("Should have nm addr");
        match cols.next().expect("Should have nm type") {
            "T" | "t" => {
                let symbol = cols.next().expect("Should have text symbol");
                let addr = String::from(addr);
                let symbol = String::from(symbol);
                text_symbols.insert(symbol, addr);
            },
            "W" => {
                let symbol = cols.next().expect("Should have text symbol");
                let addr = String::from(addr);
                let symbol = String::from(symbol);
                weak_symbols.insert(symbol, addr);
            },
            _ => ()
        };
    }

    (text_symbols, weak_symbols)
}
