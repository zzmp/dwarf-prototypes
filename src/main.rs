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

struct Flags {
    print_reserved: bool,
    print_symbols: bool,
    omit_headers: bool
}

fn print_usage(opts: &getopts::Options) -> ! { let brief = format!("Usage: {} <options> <file>...", env::args().next().unwrap());
    write!(&mut io::stderr(), "{}", opts.usage(&brief)).ok();
    process::exit(1);
}

fn main() {
    let mut opts = getopts::Options::new();
    opts.optflag("C", "print-reserved", "print reserved symbols (_FOO)");
    opts.optflag("o", "omit-headers", "print multiple files' symbols with no separators");
    opts.optflag("s", "print-symbols", "print symbols before signatures");

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

    let flags = Flags{
        print_symbols: matches.opt_present("s"),
        print_reserved: matches.opt_present("C"),
        omit_headers: matches.opt_present("o")
    };

    let mut first_file = true;
    for file_path in &matches.free {
        if matches.free.len() != 1 {
            if !flags.omit_headers {
                if !first_file {
                    println!("");
                }
                println!("{}:", file_path);
            }

            if first_file {
                first_file = false;
            }
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

        for (symbol, &&(dwarf_symbol, dwarf_signature)) in symbols.iter() {
            // skip implementer-specific functions
            if symbol.starts_with("_") {
                if !flags.print_reserved {
                    continue;
                }
            }

            // strip versioning from symbol
            let mut name = (*symbol).clone();
            match name.find("@@") {
                Some(i) => name.truncate(i),
                None => ()
            }

            let signature = if name == *dwarf_symbol {
                dwarf_signature.clone()
            } else {
                // replace signature name with symbol name
                dwarf_signature.clone().replace(dwarf_symbol, name.as_str())
            };

            if flags.print_symbols {
                println!("{}\t{}", name, signature);
            } else {
                println!("{}", signature);
            }
        }
    }
}
