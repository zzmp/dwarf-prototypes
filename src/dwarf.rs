extern crate gimli;
extern crate object;

use object::Object;
use std::option::Option;
use std::str::FromStr;

pub use self::gimli::LittleEndian as LittleEndian;
pub use self::gimli::BigEndian as BigEndian;

use std::collections::BTreeMap;

pub fn dump_file<Endian>(file: object::File) -> BTreeMap<String, String>
    where Endian: gimli::Endianity
{
    let mut symbols = BTreeMap::new();

    let debug_abbrev = file.get_section(".debug_abbrev").unwrap_or(&[]);
    let debug_abbrev = gimli::DebugAbbrev::<Endian>::new(debug_abbrev);
    let debug_str = file.get_section(".debug_str").unwrap_or(&[]);
    let debug_str = gimli::DebugStr::<Endian>::new(debug_str);

    if let Some(debug_info) = file.get_section(".debug_info") {
        let debug_info = gimli::DebugInfo::<Endian>::new(debug_info);

        let mut iter = debug_info.units();
        while let Some(unit) = iter.next().expect("Should parse compilation unit") {
            symbols.append(&mut dump_unit(unit, debug_abbrev, debug_str));
        }
    }

    symbols
}

fn dump_unit<Endian>(unit: gimli::CompilationUnitHeader<Endian>,
    debug_abbrev: gimli::DebugAbbrev<Endian>,
    debug_str: gimli::DebugStr<Endian>)
    -> BTreeMap<String, String>
    where Endian: gimli::Endianity
{
    let mut symbols = BTreeMap::new();

    let abbrevs = unit.abbreviations(debug_abbrev).expect("Should parse abbreviations");

    let mut entries = unit.entries(&abbrevs);
    while let Some((_, entry)) = entries.next_dfs().expect("Should advance DIE") {
        if entry.tag() == gimli::DW_TAG_subprogram &&
            entry.attr(gimli::DW_AT_external).expect("Should parse external").is_some() &&
            entry.attr(gimli::DW_AT_prototyped).expect("Should parse protototyped").is_some() {

            let (symbol, signature) = dump_subprogram(&unit, &abbrevs, &debug_str, entry);
            symbols.insert(symbol, signature);
        }
    }

    symbols
}

fn dump_subprogram<Endian>(unit: &gimli::CompilationUnitHeader<Endian>,
    abbrevs: &gimli::Abbreviations,
    debug_str: &gimli::DebugStr<Endian>,
    entry: &gimli::DebuggingInformationEntry<Endian>)
    -> (String, String)
    where Endian: gimli::Endianity
{
    let n = get_name(entry, debug_str);
    if n.is_empty() {
        panic!("Subprogram name should be non-empty");
    }
    let t = get_type(&unit, &abbrevs, debug_str, entry);
    let mut args = Vec::new();

    if entry.has_children() {
        let mut parameters = unit.entries_at_offset(&abbrevs, entry.offset())
            .expect("Should set parameter DIE");
        let _ = parameters.next_dfs();

        {
            let (_, parameter) = parameters.next_dfs()
                .expect("Should start parameter DIE")
                .expect("Should have parameter DIE");
            if parameter.tag() == gimli::DW_TAG_formal_parameter {
                    let n = get_name(parameter, debug_str);
                    let t = get_type(&unit, &abbrevs, debug_str, parameter);
                    args.push((n, t));
            }
        }

        while let Some(parameter) = parameters.next_sibling()
            .expect("Should advance parameter DIE") {
            if parameter.tag() == gimli::DW_TAG_formal_parameter {
                let n = get_name(parameter, debug_str);
                let t = get_type(&unit, &abbrevs, debug_str, parameter);
                args.push((n, t));
            }
        }
    }

    (n.clone(), format!("{} {}({})", t, n, args.iter().fold(String::new(), |acc, ref arg| {
        let (ref n, ref t) = **arg;
        let spacer = if n.is_empty() {
            ""
        } else {
            " "
        };
        if acc.is_empty() {
            acc + t + spacer + n
        } else {
            acc + ", " + t + spacer + n
        }
    })))
}

fn get_name<Endian>(entry: &gimli::DebuggingInformationEntry<Endian>,
    debug_str: &gimli::DebugStr<Endian>) -> String
    where Endian: gimli::Endianity
{
    let name = match entry.attr(gimli::DW_AT_name).expect("Should parse name") {
        Some(name) => {
            name.string_value(debug_str)
                .expect("Should have name")
                .to_str()
                .expect("Should validate name")
        },
        None => ""
    };
    String::from(name)
}

fn get_type<Endian>(unit: &gimli::CompilationUnitHeader<Endian>,
    abbrevs: &gimli::Abbreviations,
    debug_str: &gimli::DebugStr<Endian>,
    entry: &gimli::DebuggingInformationEntry<Endian>) -> String
    where Endian: gimli::Endianity
{
    match get_type_cursor(unit, abbrevs, debug_str, entry) {
        Some(cursor) => {
            let entry = cursor.current().expect("Should get type DIE");
            get_type_name(unit, abbrevs, debug_str, entry)
        },
        None => return String::from("void")
    }
}

fn get_type_cursor<'a, Endian>(unit: &'a gimli::CompilationUnitHeader<Endian>,
    abbrevs: &'a gimli::Abbreviations,
    _: &gimli::DebugStr<Endian>,
    entry: &gimli::DebuggingInformationEntry<Endian>)
    -> Option<gimli::EntriesCursor<'a, 'a, 'a, Endian>>
    where Endian: gimli::Endianity
{
    let offset = match entry.attr(gimli::DW_AT_type).expect("Should parse type") {
        Some(attr) => match attr.value() {
            gimli::AttributeValue::UnitRef(offset) => offset,
            _ => panic!("Should have type offset")
        },
        None => return None
    };

    let mut entries = unit.entries_at_offset(abbrevs, offset).expect("Should set type DIE");
    let _ = entries.next_dfs();
    Some(entries)
}

fn get_type_name<Endian>(unit: &gimli::CompilationUnitHeader<Endian>,
    abbrevs: &gimli::Abbreviations,
    debug_str: &gimli::DebugStr<Endian>,
    entry: &gimli::DebuggingInformationEntry<Endian>) -> String
    where Endian: gimli::Endianity
{
    // check for DW_AT_name
    let name = match entry.attr(gimli::DW_AT_name).expect("Should parse type name") {
        Some(attr) => {
            let name = attr.string_value(debug_str)
                .expect("Should have type name")
                .to_str()
                .expect("Should validate type name");
            String::from_str(name).expect("Should convert type name")
        },
        None => {
            let name = match get_type_cursor(unit, abbrevs, debug_str, entry) {
                Some(cursor) => {
                    let entry = cursor.current().expect("Should get type DIE");
                    get_type_name(unit, abbrevs, debug_str, entry)
                },
                None => String::from("void")
            };
            name + get_qualifier(entry)
        }
    };

    name
}

fn get_qualifier<Endian>(entry: &gimli::DebuggingInformationEntry<Endian>) -> &'static str
    where Endian: gimli::Endianity
{
    match entry.tag() {
        gimli::DW_TAG_pointer_type => "*",
        gimli::DW_TAG_reference_type => "&",
        gimli::DW_TAG_const_type => " const",
        gimli::DW_TAG_restrict_type => " restrict",
        gimli::DW_TAG_volatile_type => " volatile",
        // subroutines are not parsed
        gimli::DW_TAG_subroutine_type => " SUBROUTINE ",
        unknown_tag => panic!("Unknown tag: {}", unknown_tag)
    }
}
