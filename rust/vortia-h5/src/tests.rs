use super::*;

fn open(name: &str) -> File {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    File::open(&path).unwrap()
}

#[test]
fn groups_list_their_names_and_paths_are_followed() {
    let f = open("basic.h5");
    assert_eq!(f.list("/").unwrap(), ["a", "wide"]);
    assert_eq!(
        f.list("/a/b").unwrap(),
        [
            "be",
            "compact",
            "empty",
            "many",
            "names",
            "scalar",
            "u8",
            "unwritten"
        ]
    );
    assert!(f.exists("/a/f64") && !f.exists("/a/nothing"));
    assert!(matches!(f.list("/a/nothing"), Err(H5Error::NotFound(_))));
}

#[test]
fn a_group_of_hundreds_of_names_spans_several_nodes() {
    let f = open("basic.h5");
    let names = f.list("/wide").unwrap();
    assert_eq!(names.len(), 300);
    assert_eq!(names.first().map(String::as_str), Some("d000"));
    let last = f.dataset("/wide/d299").unwrap();
    assert_eq!(last.read_i64().unwrap(), [299]);
    let mid = f.dataset("/wide/d150").unwrap();
    assert_eq!(mid.read_i64().unwrap(), [150]);
}

#[test]
fn contiguous_numbers_are_read_with_their_shape() {
    let f = open("basic.h5");
    let d = f.dataset("/a/f64").unwrap();
    assert_eq!(d.shape, [2, 3]);
    assert_eq!(
        d.dtype,
        DType::Float {
            size: 8,
            big: false
        }
    );
    assert_eq!(d.read_f64().unwrap(), [0.0, 0.5, 1.0, 1.5, 2.0, 2.5]);
}

#[test]
fn a_chunked_deflated_shuffled_dataset_is_put_back_together() {
    let f = open("basic.h5");
    let d = f.dataset("/a/i32").unwrap();
    assert_eq!(d.shape, [7, 5]);
    let want: Vec<i64> = (0..35).map(|i| i - 10).collect();
    assert_eq!(d.read_i64().unwrap(), want);
    let u8s = f.dataset("/a/b/u8").unwrap().read_f64().unwrap();
    assert_eq!(u8s.len(), 1000);
    assert!(u8s.iter().enumerate().all(|(i, &v)| v == (i % 251) as f64));
}

#[test]
fn big_endian_three_dimensional_chunks_and_thousands_of_chunks_are_read() {
    let f = open("basic.h5");
    let be = f.dataset("/a/b/be").unwrap();
    assert_eq!(be.dtype, DType::Float { size: 4, big: true });
    let want: Vec<f64> = (0..27).map(f64::from).collect();
    assert_eq!(be.read_f64().unwrap(), want);
    let many = f.dataset("/a/b/many").unwrap().read_i64().unwrap();
    assert_eq!(many.len(), 4000);
    assert!(many.iter().enumerate().all(|(i, &v)| v == i as i64));
}

#[test]
fn small_odd_datasets_are_read_too() {
    let f = open("basic.h5");
    assert_eq!(
        f.dataset("/a/b/compact").unwrap().read_i64().unwrap(),
        [1, 2, 3, 4]
    );
    let scalar = f.dataset("/a/b/scalar").unwrap();
    assert!(scalar.shape.is_empty());
    assert_eq!(scalar.read_i64().unwrap(), [-7]);
    let empty = f.dataset("/a/b/empty").unwrap();
    assert!(empty.is_empty() && empty.read_f64().unwrap().is_empty());
    // Chunks never written read as zeros.
    assert_eq!(
        f.dataset("/a/b/unwritten").unwrap().read_f64().unwrap(),
        [0.0; 4]
    );
    assert_eq!(
        f.dataset("/a/b/names").unwrap().read_strings().unwrap(),
        ["inlet", "wall", "outlet"]
    );
}

#[test]
fn attributes_are_read() {
    let f = open("basic.h5");
    let units = f.attribute("/a", "units").unwrap();
    assert_eq!(units.read_strings().unwrap(), ["mm"]);
    assert_eq!(f.attribute("/a", "n").unwrap().read_i64().unwrap(), [42]);
    assert_eq!(f.attribute("/a", "x").unwrap().read_f64().unwrap(), [2.5]);
    assert_eq!(f.attributes("/a").unwrap().len(), 3);
    assert!(matches!(f.attribute("/a", "no"), Err(H5Error::NotFound(_))));
}

#[test]
fn a_newer_file_reads_what_it_can_and_names_what_it_cannot() {
    let f = open("latest.h5");
    assert_eq!(f.list("/g").unwrap(), ["c", "v"]);
    assert_eq!(
        f.dataset("/g/v").unwrap().read_f64().unwrap(),
        [1.5, 2.5, 3.5]
    );
    // Layout version 4 chunk indexes are not read yet; that is an error, not a panic.
    let chunked = f.dataset("/g/c");
    assert!(
        matches!(chunked, Err(H5Error::Unsupported(_))),
        "{:?}",
        chunked.err()
    );
    let dense = open("dense.h5");
    assert!(matches!(dense.list("/g"), Err(H5Error::Unsupported(_))));
}

#[test]
fn what_is_not_a_file_or_a_dataset_is_an_error() {
    assert!(matches!(
        File::from_bytes(b"hello".to_vec()),
        Err(H5Error::NotHdf5)
    ));
    assert!(matches!(
        File::from_bytes(Vec::new()),
        Err(H5Error::NotHdf5)
    ));
    let f = open("basic.h5");
    assert!(matches!(f.dataset("/a"), Err(H5Error::NotFound(_))));
    // A number read as text, and text as numbers, is refused.
    assert!(f.dataset("/a/f64").unwrap().read_strings().is_err());
    assert!(f.dataset("/a/b/names").unwrap().read_f64().is_err());
}

#[test]
fn a_damaged_file_never_panics() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/basic.h5");
    let good = std::fs::read(path).unwrap();
    let walk = |bytes: Vec<u8>| {
        if let Ok(f) = File::from_bytes(bytes) {
            for dir in ["/", "/a", "/a/b", "/wide"] {
                let _ = f.list(dir);
            }
            for name in [
                "/a/f64",
                "/a/i32",
                "/a/b/u8",
                "/a/b/many",
                "/a/b/be",
                "/a/b/names",
            ] {
                if let Ok(d) = f.dataset(name) {
                    let _ = d.read_f64();
                    let _ = d.read_strings();
                }
            }
            let _ = f.attributes("/a");
        }
    };
    // Cut short at many places, and with a byte flipped at many places.
    for cut in (0..good.len()).step_by(997) {
        walk(good.get(..cut).unwrap_or_default().to_vec());
    }
    for at in (0..good.len()).step_by(211) {
        for value in [0u8, 0xff, 0x80] {
            let mut bad = good.clone();
            if let Some(b) = bad.get_mut(at) {
                *b = value;
            }
            walk(bad);
        }
    }
}
