//! The boundary conditions of a field of the case, read for the Conditions tab and edited in
//! place: only the numbers of an entry change, in the field's own file, and everything else in
//! the file stays as it was.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use vortia_foamfile::dict::{Entry, Value};
use vortia_foamfile::file::FoamFile;
use vortia_foamfile::write;

use crate::controldict::{code_of, replace_in_line, set_entry};

/// One entry of a patch (or the internal field): its text, and whether it can be edited.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub key: String,
    pub text: String,
    pub original: String,
    pub editable: bool,
}

impl Item {
    pub(crate) fn new(key: &str, values: &[Value]) -> Self {
        let (text, editable) = render(values);
        Self {
            key: key.to_owned(),
            original: text.clone(),
            text,
            editable,
        }
    }

    pub(crate) fn fixed(key: &str, text: &str) -> Self {
        Self {
            key: key.to_owned(),
            text: text.to_owned(),
            original: text.to_owned(),
            editable: false,
        }
    }

    pub fn changed(&self) -> bool {
        self.text != self.original
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchConditions {
    pub patch: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldConditions {
    pub field: String,
    pub internal: Item,
    pub patches: Vec<PatchConditions>,
}

/// What the tab shows: the fields that can be chosen, and the one that was read.
#[derive(Debug)]
pub struct Loaded {
    pub fields: Vec<String>,
    pub current: Result<FieldConditions, String>,
}

/// The values of an entry as text. A list of per-cell values is not written out.
fn render(values: &[Value]) -> (String, bool) {
    if matches!(values.first(), Some(Value::Word(w)) if w == "nonuniform") {
        return ("nonuniform (a value for every cell)".to_owned(), false);
    }
    let mut text = String::new();
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            text.push(' ');
        }
        match v {
            // A short list is written with its length in front, which a person does not type.
            Value::List(items) if items.iter().all(is_number) => {
                let numbers: Vec<String> = items.iter().map(plain).collect();
                text.push_str(&format!("({})", numbers.join(" ")));
            }
            _ => write::value(&mut text, v, 0),
        }
    }
    let number = is_number;
    let editable = match values {
        [v] => number(v),
        [Value::Word(w), v] if w == "uniform" => {
            number(v)
                || matches!(v, Value::List(items) if items.len() == 3 && items.iter().all(number))
        }
        _ => false,
    };
    (text, editable)
}

fn is_number(v: &Value) -> bool {
    matches!(v, Value::Scalar(_) | Value::Label(_))
}

fn plain(v: &Value) -> String {
    let mut out = String::new();
    write::value(&mut out, v, 0);
    out
}

/// A field name is a file name in the time folder, nothing more.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && name.chars().all(|c| !c.is_control())
}

/// The names of the files of a folder.
fn list(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| valid_name(n) && !n.ends_with(".gz"))
        .collect();
    names.sort();
    names.truncate(200);
    names
}

pub fn read(case: &Path, time: &str, field: &str) -> Result<FieldConditions, String> {
    if !valid_name(field) || !valid_name(time) {
        return Err("That is not a field of the case.".to_owned());
    }
    let path = case.join(time).join(field);
    let (_, dict) = FoamFile::open(&path)
        .and_then(|f| f.dictionary())
        .map_err(|e| e.to_string())?;
    let internal = match dict.get("internalField") {
        Some(Entry::Values(vs)) => Item::new("internalField", vs),
        _ => return Err(format!("{field} has no internalField.")),
    };
    let Some(boundary) = dict.dict("boundaryField") else {
        return Err(format!("{field} has no boundaryField."));
    };
    let mut patches = Vec::new();
    for (name, entry) in boundary.entries() {
        let Entry::Dict(patch) = entry else {
            continue;
        };
        let mut items = Vec::new();
        for (key, entry) in patch.entries() {
            items.push(match entry {
                Entry::Values(vs) => Item::new(key, vs),
                Entry::Dict(_) => Item::fixed(key, "{ … }"),
                Entry::Directive(_) => Item::fixed(key, "#…"),
            });
        }
        patches.push(PatchConditions {
            patch: name.to_owned(),
            items,
        });
    }
    Ok(FieldConditions {
        field: field.to_owned(),
        internal,
        patches,
    })
}

/// Reads the folder of `time` and the field `field` (the first one when none is given), on a
/// thread of its own; `wake` is called when the answer is ready.
pub fn spawn(
    case: PathBuf,
    time: String,
    field: Option<String>,
    wake: impl Fn() + Send + 'static,
) -> Receiver<Loaded> {
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new()
        .name("vortia-conditions".into())
        .spawn(move || {
            let load = || {
                let fields = if valid_name(&time) {
                    list(&case.join(&time))
                } else {
                    Vec::new()
                };
                let current = match field.or_else(|| fields.first().cloned()) {
                    Some(name) if fields.contains(&name) => read(&case, &time, &name),
                    Some(name) => Err(format!("{name} is not in the {time} folder.")),
                    None => Err(format!("The {time} folder holds no fields.")),
                };
                Loaded { fields, current }
            };
            let loaded = std::panic::catch_unwind(load).unwrap_or_else(|_| Loaded {
                fields: Vec::new(),
                current: Err("The field could not be read.".to_owned()),
            });
            let _ = tx.send(loaded);
            wake();
        });
    if spawned.is_err() {
        let (tx, rx) = channel();
        let _ = tx.send(Loaded {
            fields: Vec::new(),
            current: Err("The field could not be read.".to_owned()),
        });
        return rx;
    }
    rx
}

/// What a typed value is: whether it says `uniform`, and how many numbers it holds.
fn shape(text: &str) -> Result<(bool, usize, String), String> {
    let text = text.trim();
    let (uniform, rest) = match text.strip_prefix("uniform") {
        Some(rest) if rest.starts_with(char::is_whitespace) => (true, rest.trim()),
        _ => (false, text),
    };
    let number = |s: &str| {
        s.parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("'{s}' is not a number."))
    };
    let (count, body) = match rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        Some(inner) => {
            let parts: Vec<&str> = inner.split_whitespace().collect();
            let numbers = parts
                .iter()
                .map(|s| number(s).map(write::scalar))
                .collect::<Result<Vec<_>, _>>()?;
            (parts.len(), format!("({})", numbers.join(" ")))
        }
        None => (1, write::scalar(number(rest)?)),
    };
    let body = if uniform {
        format!("uniform {body}")
    } else {
        body
    };
    Ok((uniform, count, body))
}

/// The text to write for `item`: the typed value, if it keeps the form of the original.
pub(crate) fn checked(item: &Item) -> Result<String, String> {
    let typed = shape(&item.text).map_err(|e| format!("{}: {e}", item.key))?;
    let before = shape(&item.original).map_err(|e| format!("{}: {e}", item.key))?;
    if (typed.0, typed.1) != (before.0, before.1) {
        return Err(format!(
            "{} must keep its form, like '{}'.",
            item.key, item.original
        ));
    }
    Ok(typed.2)
}

/// `text` with the value of `key` in the block of `patch` replaced.
fn set_in_patch(text: &str, patch: &str, key: &str, value: &str) -> Result<String, String> {
    set_at(text, &["boundaryField", patch], key, value)
}

/// `text` with the value of `key` replaced in the block reached by `path` from the top.
pub(crate) fn set_at(text: &str, path: &[&str], key: &str, value: &str) -> Result<String, String> {
    let mut stack: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    let (mut in_comment, mut in_code, mut found) = (false, false, false);
    let mut out = String::with_capacity(text.len() + value.len());
    for line in text.split_inclusive('\n') {
        let code = code_of(line, &mut in_comment);
        let t = code.trim();
        if in_code || t.contains("#{") {
            in_code = !t.contains("#}") && (in_code || t.contains("#{"));
            out.push_str(line);
            continue;
        }
        let inside = stack.iter().map(String::as_str).eq(path.iter().copied());
        if !found
            && inside
            && let Some(replaced) = replace_in_line(line, &code, key, value)?
        {
            out.push_str(&replaced);
            found = true;
            continue;
        }
        let before = t.split('{').next().unwrap_or("").trim();
        let mut name = if is_word(before) {
            Some(before.to_owned())
        } else {
            pending.take()
        };
        for c in t.chars() {
            match c {
                '{' => stack.push(name.take().unwrap_or_default()),
                '}' => {
                    stack.pop();
                }
                _ => {}
            }
        }
        pending = if !t.contains(['{', '}']) && is_word(t) {
            Some(t.to_owned())
        } else {
            None
        };
        out.push_str(line);
    }
    if found {
        Ok(out)
    } else {
        Err(format!(
            "'{key}' of {} is not written the way the editor reads; edit it in the file.",
            path.last().copied().unwrap_or("the file")
        ))
    }
}

fn is_word(t: &str) -> bool {
    !t.is_empty() && !t.contains(|c: char| c.is_whitespace() || matches!(c, ';' | '{' | '}'))
}

/// Patch types that belong to the mesh and not to the choice of a condition: the field has to
/// say what the mesh says, so a person does not pick them.
const GEOMETRIC: &[&str] = &[
    "empty",
    "wedge",
    "cyclic",
    "cyclicAMI",
    "processor",
    "symmetryPlane",
];

/// Whether the vector in `field` is one (U) and not a scalar.
fn is_vector(field: &FieldConditions) -> bool {
    let text = field.internal.original.trim();
    match text.strip_prefix("uniform") {
        Some(rest) => rest.trim_start().starts_with('('),
        None => field.field == "U",
    }
}

/// Whether the patch's condition can be picked from a list: not one the mesh decides.
pub fn retypable(patch: &PatchConditions) -> bool {
    patch
        .items
        .iter()
        .find(|i| i.key == "type")
        .is_some_and(|i| !GEOMETRIC.contains(&i.original.as_str()))
}

/// The condition types the engine has for the field, for the application the case runs: what a
/// person may pick for a patch that the mesh does not decide.
pub fn patch_types(field: &FieldConditions, application: Option<&str>) -> Vec<&'static str> {
    if is_vector(field) {
        let mut types = vec!["fixedValue", "noSlip", "zeroGradient"];
        if matches!(application, Some("icoFoam" | "simpleFoam")) {
            types.extend(["slip", "symmetry"]);
        }
        types
    } else {
        vec!["fixedValue", "zeroGradient", "fixedGradient"]
    }
}

/// The lines of a block for `kind`, with what it needs to be valid.
fn block_for(field: &FieldConditions, kind: &str) -> Result<Vec<(&'static str, String)>, String> {
    let vector = is_vector(field);
    let start = if vector {
        "uniform (0 0 0)".to_owned()
    } else {
        let internal = field.internal.original.trim();
        match internal.strip_prefix("uniform") {
            Some(v) if v.trim().parse::<f64>().is_ok_and(f64::is_finite) => {
                format!("uniform {}", v.trim())
            }
            _ => "uniform 0".to_owned(),
        }
    };
    let mut lines: Vec<(&'static str, String)> = Vec::new();
    match kind {
        "fixedValue" => lines.push(("value", start)),
        "zeroGradient" => {}
        "noSlip" | "slip" | "symmetry" if vector => {}
        "fixedGradient" if !vector => {
            lines.push(("gradient", "uniform 0".to_owned()));
            lines.push(("value", start));
        }
        other => return Err(format!("'{other}' cannot be set for {}.", field.field)),
    }
    lines.insert(0, ("type", kind.to_owned()));
    Ok(lines)
}

/// `text` with the block of `patch` written again from `lines`, keeping where it stands.
fn replace_patch(text: &str, patch: &str, lines: &[(&str, String)]) -> Result<String, String> {
    let source: Vec<&str> = text.split_inclusive('\n').collect();
    let mut stack: Vec<String> = Vec::new();
    let mut pending: Option<(String, usize)> = None;
    let (mut in_comment, mut in_code) = (false, false);
    let (mut start, mut end): (Option<usize>, Option<usize>) = (None, None);
    for (i, line) in source.iter().enumerate() {
        let code = code_of(line, &mut in_comment);
        let t = code.trim();
        if in_code || t.contains("#{") {
            in_code = !t.contains("#}") && (in_code || t.contains("#{"));
            continue;
        }
        let before = t.split('{').next().unwrap_or("").trim();
        let mut name = if is_word(before) {
            Some((before.to_owned(), i))
        } else {
            pending.take()
        };
        for c in t.chars() {
            match c {
                '{' => {
                    let named = name.take();
                    if start.is_none()
                        && stack.len() == 1
                        && stack.first().is_some_and(|s| s == "boundaryField")
                        && let Some((n, at)) = &named
                        && n == patch
                    {
                        start = Some(*at);
                    }
                    stack.push(named.map(|(n, _)| n).unwrap_or_default());
                }
                '}' => {
                    stack.pop();
                    if start.is_some() && end.is_none() && stack.len() == 1 {
                        end = Some(i);
                    }
                }
                _ => {}
            }
        }
        pending = if !t.contains(['{', '}']) && is_word(t) {
            Some((t.to_owned(), i))
        } else {
            None
        };
        if end.is_some() {
            break;
        }
    }
    let (Some(start), Some(end)) = (start, end) else {
        return Err(format!(
            "The block of {patch} is not written the way the editor reads; edit it in the file."
        ));
    };
    let first = source.get(start).copied().unwrap_or("");
    let indent: String = first.chars().take_while(|c| c.is_whitespace()).collect();
    let mut out = String::with_capacity(text.len());
    for line in source.iter().take(start) {
        out.push_str(line);
    }
    out.push_str(&format!("{indent}{patch}\n{indent}{{\n"));
    for (key, value) in lines {
        out.push_str(&format!("{indent}    {key:<15} {value};\n"));
    }
    out.push_str(&format!("{indent}}}\n"));
    for line in source.iter().skip(end + 1) {
        out.push_str(line);
    }
    Ok(out)
}

/// The text of the field's file with every edited item of `conditions` applied.
pub fn apply(text: &str, conditions: &FieldConditions) -> Result<String, String> {
    let mut text = text.to_owned();
    if conditions.internal.changed() {
        text = set_entry(&text, "internalField", &checked(&conditions.internal)?)?;
    }
    for patch in &conditions.patches {
        // A new condition type starts the patch's block again: what the old type needed may not
        // be what the new one does, and its other edits go with it.
        if let Some(kind) = patch
            .items
            .iter()
            .find(|i| i.key == "type" && i.changed())
            .map(|i| i.text.clone())
        {
            if !retypable(patch) {
                return Err(format!(
                    "The condition type of {} is decided by the mesh.",
                    patch.patch
                ));
            }
            let lines = block_for(conditions, &kind)?;
            text = replace_patch(&text, &patch.patch, &lines)?;
            continue;
        }
        for item in patch.items.iter().filter(|i| i.editable && i.changed()) {
            text = set_in_patch(&text, &patch.patch, &item.key, &checked(item)?)?;
        }
    }
    Ok(text)
}

/// Writes the edits of `conditions` into the field's file in the `time` folder, atomically.
pub fn save(case: &Path, time: &str, conditions: &FieldConditions) -> Result<(), String> {
    if !valid_name(time) || !valid_name(&conditions.field) {
        return Err("That is not a field of the case.".to_owned());
    }
    let path = case.join(time).join(&conditions.field);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let edited = apply(&text, conditions)?;
    write::atomic(&path, edited.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "\
FoamFile
{
    version     2.0;
    format      ascii;
    class       volScalarField;
    object      T;
}
dimensions      [0 0 0 1 0 0 0];
internalField   uniform 300;
boundaryField
{
    axis
    {
        type            empty;
    }
    inlet
    {
        type            totalTemperature;
        T0              uniform 300; // the plenum
        gamma           1.4;
        value           uniform 300;
    }
    atmosphere
    {
        type            inletOutlet;
        inletValue      uniform 300;
        value           uniform 300;
    }
}
";

    fn case_with(text: &str) -> tempfile::TempDir {
        case_with_field("T", text)
    }

    fn case_with_field(name: &str, text: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("0")).unwrap();
        std::fs::write(dir.path().join("0").join(name), text).unwrap();
        dir
    }

    #[test]
    fn a_field_is_read_with_what_can_be_edited() {
        let case = case_with(T);
        let read = read(case.path(), "0", "T").unwrap();
        assert_eq!(read.internal.text, "uniform 300");
        assert!(read.internal.editable);
        let inlet = &read.patches[1];
        assert_eq!(inlet.patch, "inlet");
        let find = |key: &str| inlet.items.iter().find(|i| i.key == key).unwrap();
        assert!(!find("type").editable);
        assert_eq!(find("type").text, "totalTemperature");
        assert!(find("gamma").editable);
        assert_eq!(find("T0").text, "uniform 300");
    }

    #[test]
    fn only_the_edited_values_change_in_the_file() {
        let case = case_with(T);
        let mut read = read(case.path(), "0", "T").unwrap();
        read.internal.text = "uniform 310".into();
        let inlet = &mut read.patches[1];
        inlet.items[1].text = "uniform 450".into(); // T0
        inlet.items[2].text = "1.3".into(); // gamma
        save(case.path(), "0", &read).unwrap();
        let text = std::fs::read_to_string(case.path().join("0/T")).unwrap();
        assert_eq!(
            text,
            T.replace("internalField   uniform 300", "internalField   uniform 310")
                .replace(
                    "T0              uniform 300;",
                    "T0              uniform 450;"
                )
                .replace("gamma           1.4", "gamma           1.3")
        );
        // The atmosphere's values are the same text as the inlet's, and stay.
        assert_eq!(text.matches("inletValue      uniform 300;").count(), 1);
    }

    #[test]
    fn a_value_keeps_its_form() {
        let mut item = Item::new("U", &[]);
        item.original = "uniform (1 0 0)".into();
        item.editable = true;
        item.text = "uniform (2 0 0)".into();
        assert_eq!(checked(&item).unwrap(), "uniform (2 0 0)");
        for bad in [
            "uniform 2",
            "(2 0 0)",
            "uniform (2 0)",
            "uniform (a 0 0)",
            "uniform (inf 0 0)",
            "",
        ] {
            item.text = bad.into();
            assert!(checked(&item).is_err(), "{bad}");
        }
    }

    const U: &str = "\
FoamFile { version 2.0; format ascii; class volVectorField; object U; }
dimensions      [0 1 -1 0 0 0 0];
internalField   uniform (1 0 0);
boundaryField
{
    inlet { type fixedValue; value uniform (1 0 0); }
    wall
    {
        type            noSlip;
    }
    top // the lid
    {
        type            zeroGradient;
    }
    front
    {
        type            empty;
    }
}
";

    #[test]
    fn a_patch_can_be_given_another_condition_type_and_keeps_the_rest_of_the_file() {
        let case = case_with_field("U", U);
        let mut read = read(case.path(), "0", "U").unwrap();
        let set = |read: &mut FieldConditions, patch: &str, kind: &str| {
            let patch = read.patches.iter_mut().find(|p| p.patch == patch).unwrap();
            patch
                .items
                .iter_mut()
                .find(|i| i.key == "type")
                .unwrap()
                .text = kind.into();
        };
        set(&mut read, "wall", "slip");
        set(&mut read, "inlet", "zeroGradient");
        set(&mut read, "top", "fixedValue");
        save(case.path(), "0", &read).unwrap();
        let text = std::fs::read_to_string(case.path().join("0/U")).unwrap();
        let again = super::read(case.path(), "0", "U").unwrap();
        let kind = |name: &str| {
            let patch = again.patches.iter().find(|p| p.patch == name).unwrap();
            let items: Vec<_> = patch
                .items
                .iter()
                .map(|i| (i.key.as_str(), i.text.as_str()))
                .collect();
            items
        };
        assert_eq!(kind("wall"), [("type", "slip")]);
        assert_eq!(kind("inlet"), [("type", "zeroGradient")]);
        assert_eq!(
            kind("top"),
            [("type", "fixedValue"), ("value", "uniform (0 0 0)")]
        );
        assert_eq!(kind("front"), [("type", "empty")]);
        assert!(text.starts_with("FoamFile { version 2.0;"), "{text}");
        assert!(text.contains("internalField   uniform (1 0 0);"));
        // Nothing of the old blocks is left behind.
        assert!(!text.contains("noSlip"));
    }

    #[test]
    fn a_scalar_type_that_needs_a_value_starts_from_the_internal_one() {
        let case = case_with(T);
        let mut read = read(case.path(), "0", "T").unwrap();
        let inlet = read
            .patches
            .iter_mut()
            .find(|p| p.patch == "inlet")
            .unwrap();
        inlet
            .items
            .iter_mut()
            .find(|i| i.key == "type")
            .unwrap()
            .text = "fixedGradient".into();
        // Other edits of that patch are dropped with the old block.
        inlet
            .items
            .iter_mut()
            .find(|i| i.key == "gamma")
            .unwrap()
            .text = "1.3".into();
        save(case.path(), "0", &read).unwrap();
        let again = super::read(case.path(), "0", "T").unwrap();
        let inlet = again.patches.iter().find(|p| p.patch == "inlet").unwrap();
        let items: Vec<_> = inlet
            .items
            .iter()
            .map(|i| (i.key.as_str(), i.text.as_str()))
            .collect();
        assert_eq!(
            items,
            [
                ("type", "fixedGradient"),
                ("gradient", "uniform 0"),
                ("value", "uniform 300")
            ]
        );
        let atmosphere = again
            .patches
            .iter()
            .find(|p| p.patch == "atmosphere")
            .unwrap();
        assert_eq!(atmosphere.items.len(), 3);
    }

    #[test]
    fn what_the_mesh_decides_is_not_picked_and_the_list_follows_the_engine() {
        let case = case_with_field("U", U);
        let mut read = read(case.path(), "0", "U").unwrap();
        let front = read
            .patches
            .iter()
            .position(|p| p.patch == "front")
            .unwrap();
        assert!(!retypable(&read.patches[front]));
        assert!(retypable(&read.patches[0]));
        read.patches[front].items[0].text = "fixedValue".into();
        let err = save(case.path(), "0", &read).unwrap_err();
        assert!(err.contains("decided by the mesh"), "{err}");
        let incompressible = patch_types(&read, Some("simpleFoam"));
        assert!(incompressible.contains(&"slip") && incompressible.contains(&"symmetry"));
        let compressible = patch_types(&read, Some("rhoSimpleFoam"));
        assert!(!compressible.contains(&"slip"));
        let scalar = super::read(case_with(T).path(), "0", "T").unwrap();
        assert_eq!(
            patch_types(&scalar, None),
            ["fixedValue", "zeroGradient", "fixedGradient"]
        );
    }

    #[test]
    fn a_vector_and_a_nonuniform_field_are_read() {
        let case = case_with_field(
            "U",
            "FoamFile { version 2.0; format ascii; class volVectorField; object U; }\n\
             dimensions [0 1 -1 0 0 0 0];\n\
             internalField nonuniform List<vector> 2((0 0 0)(1 0 0));\n\
             boundaryField\n{\n    in\n    {\n        type fixedValue;\n        value uniform (1 0 0);\n    }\n}\n",
        );
        let read = read(case.path(), "0", "U").unwrap();
        assert!(!read.internal.editable);
        assert_eq!(read.patches[0].items[1].text, "uniform (1 0 0)");
        assert!(read.patches[0].items[1].editable);
    }

    #[test]
    fn what_cannot_be_found_or_read_is_an_error() {
        let case = case_with(T);
        assert!(read(case.path(), "0", "nothing").is_err());
        assert!(read(case.path(), "0", "../0/T").is_err());
        assert!(read(case.path(), "..", "T").is_err());
        assert!(set_in_patch(T, "inlet", "nothing", "1").is_err());
        assert!(set_in_patch(T, "outlet", "value", "1").is_err());
        let broken = case_with("not a dictionary {{{");
        assert!(read(broken.path(), "0", "T").is_err());
    }

    #[test]
    fn the_thread_lists_the_folder_and_reads_the_first_field() {
        let case = case_with(T);
        let rx = spawn(case.path().to_path_buf(), "0".into(), None, || {});
        let loaded = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(loaded.fields, ["T"]);
        assert_eq!(loaded.current.unwrap().field, "T");
        let rx = spawn(case.path().to_path_buf(), "7".into(), None, || {});
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(10))
                .unwrap()
                .current
                .is_err()
        );
    }
}
