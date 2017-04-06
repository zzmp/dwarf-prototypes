extern crate getopts;
extern crate memmap;
extern crate object;

mod dwarf;
mod objfile;

use object::Object;
use std::collections::BTreeMap;
use std::env;
use std::io::Write;
use std::io;
use std::fs;
use std::process;

fn print_usage(opts: &getopts::Options) -> ! { let brief = format!("Usage: {} <options> <file>...", env::args().next().unwrap());
    write!(&mut io::stderr(), "{}", opts.usage(&brief)).ok();
    process::exit(1);
}

fn main() {
    let opts = getopts::Options::new();
    let matches = match opts.parse(env::args().skip(1)) {
        Ok(m) => m,
        Err(e) => {
            writeln!(&mut io::stderr(), "{:?}\n", e).ok();
            print_usage(&opts);
        }
    };
    if matches.free.is_empty() {
        print_usage(&opts);
    }

    for file_path in &matches.free {
        if matches.free.len() != 1 {
            println!("{}", file_path);
            println!("");
        }

        let file = fs::File::open(&file_path)
            .expect("Should open file");
        let file = memmap::Mmap::open(&file, memmap::Protection::Read)
            .expect("Should create mmap for file");
        let file = object::File::parse(unsafe { file.as_slice() })
            .expect("Should parse object file");

        // dump the subprogram symbols and their signatures from DWARF
        let dwarf_symbols = if file.is_little_endian() {
            dwarf::dump_file::<dwarf::LittleEndian>(file)
        } else {
            dwarf::dump_file::<dwarf::BigEndian>(file)
        };

        // dump the global weak (W) and all text (t, T) symbols from object files
        let (text_symbols, weak_symbols)= objfile::dump_file(file_path);

        // associate dwarf symbols with their addrs
        let mut dwarf_addrs = BTreeMap::new();
        for (symbol, addr) in text_symbols.iter() {
            if dwarf_symbols.contains_key(symbol) {
                dwarf_addrs.insert(addr, (symbol,  dwarf_symbols.get(symbol).unwrap()));
            }
        }

        let mut symbols = BTreeMap::new();
        for (symbol, addr) in text_symbols.iter() {
            // skip versioned symbols except the default
            // signatures should be consistent across versions
            if symbol.contains("@") && !symbol.contains("@@") {
                continue;
            }

            if dwarf_addrs.contains_key(addr) {
                symbols.insert(symbol, dwarf_addrs.get(addr).unwrap());
            }
        }
        for (symbol, addr) in weak_symbols.iter() {
            if dwarf_addrs.contains_key(addr) {
                symbols.insert(symbol, dwarf_addrs.get(addr).unwrap());
            }
        }

        for (symbol, &&(dwarf_symbol, signature)) in symbols.iter() {
            // skip implementer-specific functions
            if symbol.starts_with("_") {
                continue;
            }

            // strip versioning from symbol
            let mut name = (*symbol).clone();
            match name.find("@@") {
                Some(i) => name.truncate(i),
                None => ()
            }

            if name == *dwarf_symbol {
                println!("{}: {}", name, signature);
            } else {
                // replace signature name with symbol name
                let updated_signature = signature.clone()
                    .replace(dwarf_symbol, name.as_str());
                println!("{}: {}", name, updated_signature);
            }
        }
    }
}
