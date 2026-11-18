//! A Fluent case in the common file format (`.cas.h5`, `.msh.h5`) as the files of a `polyMesh`.
//!
//! The mesh is read from the HDF5 file (`vortia_h5`, our own reader) into what the `.msh` reader
//! gathers, and the same conversion then builds the mesh, so a case and its `.msh` give the same
//! result. The layout follows `meshes/<n>/...`: `nodes/coords/<zone>` (points, one row each),
//! `faces/nodes/<zone>/{nnodes,nodes}` with `faces/c0/<zone>` and `faces/c1/<zone>` (the cells
//! each face separates, counted from one), `cells/ctype/<zone>` for mixed cells, and a
//! `zoneTopology` group under `nodes`, `faces` and `cells` with the zones' ids, first and last
//! number, type and name.
//!
//! Written from the published description of the format and checked on files this project
//! made; a dataset that is not where it is looked for is named in the error, together with
//! what the group holds, so that a file laid out differently says how.

use std::path::Path;

use vortia_core::Vec3;
use vortia_foamfile::RawPolyMesh;
use vortia_h5::{Dataset, File, H5Error};

use crate::error::{MeshError, Result};
use crate::fluent::{Fluent, Options, convert, validate};

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(MeshError::Invalid(message.into()))
}

fn from_h5(e: H5Error) -> MeshError {
    MeshError::Invalid(format!("the HDF5 file: {e}"))
}

/// The name of a face zone type, by Fluent's number.
fn face_zone_type(code: i64) -> &'static str {
    match code {
        2 => "interior",
        3 => "wall",
        4 => "pressure-inlet",
        5 => "pressure-outlet",
        7 => "symmetry",
        9 => "pressure-far-field",
        10 => "velocity-inlet",
        14 => "fan",
        20 => "mass-flow-inlet",
        24 => "interface",
        36 => "outflow",
        37 => "axis",
        // Periodic, shadow and hanging-node zones carry no boundary of their own.
        _ => "",
    }
}

fn dataset<'f>(file: &'f File, path: &str) -> Result<Dataset<'f>> {
    file.dataset(path).map_err(|e| match e {
        H5Error::NotFound(_) => {
            let parent = path.rsplit_once('/').map_or("/", |(p, _)| p);
            let held = file.list(parent).unwrap_or_default().join(", ");
            MeshError::Invalid(format!(
                "the case has no {path} (the group holds: {})",
                if held.is_empty() { "nothing" } else { &held }
            ))
        }
        other => from_h5(other),
    })
}

fn ints(file: &File, path: &str) -> Result<Vec<i64>> {
    dataset(file, path)?.read_i64().map_err(from_h5)
}

/// The zone ids of a group, in order: its children are named by them.
fn zone_ids(file: &File, group: &str) -> Result<Vec<i64>> {
    let mut ids: Vec<i64> = file
        .list(group)
        .map_err(from_h5)?
        .iter()
        .filter_map(|n| n.parse().ok())
        .collect();
    ids.sort_unstable();
    Ok(ids)
}

/// A zone's first and last number, type and name, from a `zoneTopology` group.
struct Zones {
    id: Vec<i64>,
    min: Vec<i64>,
    max: Vec<i64>,
    kind: Vec<i64>,
    name: Vec<String>,
}

impl Zones {
    fn read(file: &File, group: &str) -> Result<Self> {
        let at = |leaf: &str| format!("{group}/zoneTopology/{leaf}");
        let id = ints(file, &at("id"))?;
        let min = ints(file, &at("minId"))?;
        let max = ints(file, &at("maxId"))?;
        let kind = if file.exists(&at("zoneType")) {
            ints(file, &at("zoneType"))?
        } else {
            Vec::new()
        };
        let name = if file.exists(&at("name")) {
            dataset(file, &at("name"))?
                .read_strings()
                .map_err(from_h5)?
        } else {
            Vec::new()
        };
        if min.len() != id.len() || max.len() != id.len() {
            return invalid(format!(
                "{group}/zoneTopology lists zones of different counts"
            ));
        }
        Ok(Self {
            id,
            min,
            max,
            kind,
            name,
        })
    }

    fn rows(&self) -> impl Iterator<Item = (usize, i64, i64, i64)> + '_ {
        (0..self.id.len())
            .filter_map(|i| Some((i, *self.id.get(i)?, *self.min.get(i)?, *self.max.get(i)?)))
    }
}

/// What a case holds, as the `.msh` reader gathers it.
pub(crate) fn read(file: &File) -> Result<Fluent> {
    let mesh = file
        .list("/meshes")
        .map_err(from_h5)?
        .into_iter()
        .find(|n| n.parse::<u32>().is_ok())
        .map(|n| format!("/meshes/{n}"))
        .ok_or_else(|| MeshError::Invalid("the file has no mesh under /meshes".into()))?;
    let mut f = Fluent::default();

    // Points: each zone's rows, in the order of their first number.
    let mut coords: Vec<(i64, i64)> = Vec::new();
    let node_zones = Zones::read(file, &format!("{mesh}/nodes")).ok();
    for id in zone_ids(file, &format!("{mesh}/nodes/coords"))? {
        let first = node_zones
            .as_ref()
            .and_then(|z| z.rows().find(|r| r.1 == id).map(|r| r.2))
            .unwrap_or(id);
        coords.push((first, id));
    }
    coords.sort_unstable();
    for (_, id) in coords {
        let data = dataset(file, &format!("{mesh}/nodes/coords/{id}"))?;
        let components = match data.shape.as_slice() {
            [_, c] if *c == 2 || *c == 3 => usize::try_from(*c).unwrap_or(3),
            _ => {
                return invalid(format!(
                    "the points of zone {id} are not rows of 2 or 3 numbers"
                ));
            }
        };
        if f.dimension == 0 {
            f.dimension = components as i64;
        }
        let values = data.read_f64().map_err(from_h5)?;
        for row in values.chunks(components) {
            let at = |i: usize| row.get(i).copied().unwrap_or(0.0);
            f.points.push(Vec3::new(at(0), at(1), at(2)));
        }
    }
    if f.points.is_empty() {
        return invalid("the case has no points");
    }

    // Faces, numbered across the zones as the file numbers them.
    let faces = Zones::read(file, &format!("{mesh}/faces"))?;
    let n_faces = faces
        .rows()
        .map(|r| r.3)
        .max()
        .and_then(|m| usize::try_from(m).ok())
        .ok_or_else(|| MeshError::Invalid("the case has no faces".into()))?;
    if n_faces > 1 << 28 {
        return invalid("the case has too many faces");
    }
    f.faces = vec![Vec::new(); n_faces];
    f.owner = vec![-1; n_faces];
    f.neighbour = vec![-1; n_faces];
    for (row, id, first, last) in faces.rows() {
        let at = |leaf: &str| format!("{mesh}/faces/{leaf}/{id}");
        let count = usize::try_from(last - first + 1)
            .map_err(|_| MeshError::Invalid(format!("face zone {id} ends before it starts")))?;
        let first_face = usize::try_from(first - 1)
            .map_err(|_| MeshError::Invalid(format!("face zone {id} starts at {first}")))?;
        let sizes = ints(file, &format!("{mesh}/faces/nodes/{id}/nnodes"))?;
        let labels = ints(file, &format!("{mesh}/faces/nodes/{id}/nodes"))?;
        let c0 = ints(file, &at("c0"))?;
        let c1 = ints(file, &at("c1"))?;
        if sizes.len() != count || c0.len() != count || c1.len() != count {
            return invalid(format!(
                "face zone {id} holds a different number of faces than it declares"
            ));
        }
        let mut next = labels.iter();
        for (k, &size) in sizes.iter().enumerate() {
            let size = usize::try_from(size)
                .ok()
                .filter(|&s| (3..=labels.len().max(3)).contains(&s))
                .ok_or_else(|| {
                    MeshError::Invalid(format!("face {} has a bad size", first_face + k + 1))
                })?;
            let mut vertices = Vec::with_capacity(size);
            for _ in 0..size {
                let label = next.next().copied().ok_or_else(|| {
                    MeshError::Invalid(format!(
                        "face zone {id} has fewer points than its faces need"
                    ))
                })?;
                vertices.push(usize::try_from(label - 1).map_err(|_| {
                    MeshError::Invalid(format!("face {} has a point {label}", first_face + k + 1))
                })?);
            }
            let slot = first_face + k;
            if let (Some(v), Some(n), Some(o), Some(&a), Some(&b)) = (
                f.faces.get_mut(slot),
                f.neighbour.get_mut(slot),
                f.owner.get_mut(slot),
                c0.get(k),
                c1.get(k),
            ) {
                *v = vertices;
                // As in the `.msh` file: the first cell is the neighbour, the second the owner.
                *n = a - 1;
                *o = b - 1;
            }
        }
        f.face_groups.push((id, first, last));
        let type_name = faces.kind.get(row).map_or("", |&c| face_zone_type(c));
        f.zone_type.insert(id, type_name.to_owned());
        let name = faces
            .name
            .get(row)
            .map(|n| validate(n))
            .filter(|n| !n.is_empty());
        f.zone_name
            .insert(id, name.unwrap_or_else(|| format!("zone_{id}")));
    }

    // Cells: one type each.
    let cells = Zones::read(file, &format!("{mesh}/cells"))?;
    let n_cells = cells
        .rows()
        .map(|r| r.3)
        .max()
        .and_then(|m| usize::try_from(m).ok())
        .ok_or_else(|| MeshError::Invalid("the case has no cells".into()))?;
    if n_cells > 1 << 28 {
        return invalid("the case has too many cells");
    }
    f.model = vec![0; n_cells];
    let uniform = file.exists(&format!("{mesh}/cells/zoneTopology/cellType"));
    for (row, id, first, last) in cells.rows() {
        let wanted = usize::try_from(first - 1)
            .ok()
            .zip(usize::try_from(last).ok())
            .filter(|(a, b)| a <= b && *b <= n_cells)
            .ok_or_else(|| MeshError::Invalid(format!("cell zone {id} has a bad range")))?;
        let types = format!("{mesh}/cells/ctype/{id}");
        if file.exists(&types) {
            let mixed = ints(file, &types)?;
            if mixed.len() != wanted.1 - wanted.0 {
                return invalid(format!(
                    "cell zone {id} holds a different number of cells than it declares"
                ));
            }
            for (slot, &m) in f.model.iter_mut().skip(wanted.0).zip(&mixed) {
                *slot = m;
            }
        } else if uniform {
            let element = ints(file, &format!("{mesh}/cells/zoneTopology/cellType"))?
                .get(row)
                .copied()
                .unwrap_or(0);
            for slot in f.model.iter_mut().take(wanted.1).skip(wanted.0) {
                *slot = element;
            }
        } else {
            return invalid(format!(
                "the case does not say the type of the cells of zone {id} (no {types} nor cellType)"
            ));
        }
        f.cell_groups.push((id, first, last));
        let name = cells
            .name
            .get(row)
            .map(|n| validate(n))
            .filter(|n| !n.is_empty());
        f.zone_name
            .insert(id, name.unwrap_or_else(|| format!("zone_{id}")));
        f.zone_type.insert(id, "fluid".to_owned());
    }
    Ok(f)
}

/// A Fluent case or mesh in the common file format as the mesh `fluentMeshToFoam` makes of it.
pub fn cff_to_foam(path: &Path, options: Options) -> Result<RawPolyMesh> {
    let file = File::open(path).map_err(from_h5)?;
    convert(read(&file)?, options)
}
