extern crate gimli;
extern crate object;

use object::Object;
use std::option::Option;

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
    let name = match get_name(entry, debug_str) {
        Some(name) => name,
        None => unreachable!()
    };
    let typed_name = get_parameter(&unit, &abbrevs, debug_str, entry);
    let parameters = dump_parameters(unit, abbrevs, debug_str, entry);
    let signature = format!("{}{}", typed_name, parameters);
    (name, signature)
}

fn dump_parameters<Endian>(unit: &gimli::CompilationUnitHeader<Endian>,
    abbrevs: &gimli::Abbreviations,
    debug_str: &gimli::DebugStr<Endian>,
    entry: &gimli::DebuggingInformationEntry<Endian>)
    -> String
    where Endian: gimli::Endianity
{
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
                args.push(get_parameter(&unit, &abbrevs, debug_str, parameter));
            }
        }

        while let Some(parameter) = parameters.next_sibling()
            .expect("Should advance parameter DIE") {
            if parameter.tag() == gimli::DW_TAG_formal_parameter {
                args.push(get_parameter(&unit, &abbrevs, debug_str, parameter));
            }
        }
    }

    let parameters = args.iter().fold(String::new(), |parameters, ref parameter| {
        if parameters.is_empty() {
            parameters + parameter.as_str()
        } else {
            parameters + ", " + parameter.as_str()
        }
    });
    format!("({})", parameters)
}

fn get_name<Endian>(entry: &gimli::DebuggingInformationEntry<Endian>,
    debug_str: &gimli::DebugStr<Endian>) -> Option<String>
    where Endian: gimli::Endianity
{
    match entry.attr(gimli::DW_AT_name).expect("Should parse name") {
        Some(name) => {
            let name = name.string_value(debug_str).expect("Should have name")
                .to_str().expect("Should validate name");
            let name = String::from(name);
            Some(name)
        },
        None => None
    }
}

fn get_parameter<Endian>(unit: &gimli::CompilationUnitHeader<Endian>,
    abbrevs: &gimli::Abbreviations,
    debug_str: &gimli::DebugStr<Endian>,
    entry: &gimli::DebuggingInformationEntry<Endian>) -> String
    where Endian: gimli::Endianity
{
    let name = get_name(entry, debug_str);

    let mut offset = get_type_offset(entry);
    let mut subroutine_offset = None;
    let mut type_name = match offset {
        Some(_) => None,
        None => Some(String::from("void"))
    };

    let mut specs = Vec::new();

    // find all specifiers on the way to the name
    while type_name == None {
        let mut cursor = unit.entries_at_offset(abbrevs, offset.unwrap())
            .expect("Should set type DIE");
        let _ = cursor.next_dfs();
        let entry = cursor.current().expect("Should have type DIE");
        type_name = get_name(entry, debug_str);
        if type_name == None {
            let specifier = get_type_specifier(entry);
            // subroutines need special treatment
            if specifier == Qualifier::Subroutine {
                specs.pop(); // all subroutines are pointers
                subroutine_offset = offset;
            }
            specs.push(specifier);
            offset = get_type_offset(entry);
            if offset == None {
                type_name = Some(String::from("void"));
            }
        };
    }

    // build the decl-specifier-seq
    let parameter = specs.iter().fold(type_name.unwrap(), |seq, spec| {
        match *spec {
            Qualifier::Specifier(spec) => seq + spec,
            Qualifier::Attr(attr) => seq + " " + attr,
            Qualifier::Subroutine => seq
        }
    });

    // subroutines have unique syntax
    if specs.first() == Some(&Qualifier::Subroutine) {
        let mut cursor = unit.entries_at_offset(abbrevs, subroutine_offset.unwrap())
            .expect("Should set type DIE");
        let _ = cursor.next_dfs();
        let entry = cursor.current().expect("Should have type DIE");
        let subparameters = dump_parameters(unit, abbrevs, debug_str, entry);
        let name = name.unwrap_or(String::from(""));
        format!("{} (*{}){}", parameter, name.as_str(), subparameters)
    } else {
        match name {
            Some(name) => format!("{} {}", parameter, name),
            None => parameter
        }
    }
}

fn get_type_offset<Endian>(entry: &gimli::DebuggingInformationEntry<Endian>)
    -> Option<gimli::UnitOffset>
    where Endian: gimli::Endianity
{
    match entry.attr(gimli::DW_AT_type).expect("Should parse type") {
        Some(attr) => match attr.value() {
            gimli::AttributeValue::UnitRef(offset) => Some(offset),
            _ => unreachable!()
        },
        None => None
    }
}

#[derive(Debug)]
#[derive(PartialEq)]
enum Qualifier {
    Specifier(&'static str),
    Attr(&'static str),
    Subroutine
}

fn get_type_specifier<Endian>(entry: &gimli::DebuggingInformationEntry<Endian>) -> Qualifier
    where Endian: gimli::Endianity
{
    match entry.tag() {
        gimli::DW_TAG_pointer_type => Qualifier::Specifier("*"),
        gimli::DW_TAG_reference_type => Qualifier::Specifier("&"),
        gimli::DW_TAG_subroutine_type => Qualifier::Subroutine,
        gimli::DW_TAG_const_type => Qualifier::Attr("const"),
        gimli::DW_TAG_volatile_type => Qualifier::Attr("volatile"),
        gimli::DW_TAG_restrict_type => Qualifier::Attr("restrict"),
        unknown_tag => panic!("Unknown tag: {}", unknown_tag)
    }
}
