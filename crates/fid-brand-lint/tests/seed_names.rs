//! REQ-OUTR.BRAND.LINT-007 / AC-9: every product named on the title line of a competitor
//! inventory in `seeds/*.md` is in `rules/incumbents.txt`.
//!
//! A title line is the first `# ` line. Products are the parts separated by " / ", with any
//! trailing parenthetical (module or edition names of the same product) removed. A part counts
//! as listed when it starts with a listed name on a word boundary (so "ZEISS INSPECT Optical 3D"
//! is covered by "ZEISS INSPECT"). `devices.md` and `journeys.md` are not competitor
//! inventories (their titles name no product) and are skipped along with `README.md` and
//! `out-of-scope.md`; any new seed file is treated as a competitor inventory.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

const NOT_COMPETITOR_INVENTORIES: [&str; 4] = ["README.md", "out-of-scope.md", "devices.md", "journeys.md"];

fn incumbents() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("rules/incumbents.txt");
    let text = std::fs::read_to_string(&path).unwrap();
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_string).collect()
}

fn title_products(title: &str) -> Vec<String> {
    let t = match title.find('(') {
        Some(i) => &title[..i],
        None => title,
    };
    t.split(" / ").map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

fn covered(product: &str, names: &[String]) -> bool {
    names.iter().any(|n| {
        product.starts_with(n.as_str())
            && product[n.len()..].chars().next().map_or(true, |c| !c.is_alphanumeric())
    })
}

#[test]
fn title_parsing_is_hand_checked() {
    assert_eq!(title_products("Alpha Gauge / Beta Scan Pro 3D"), vec!["Alpha Gauge", "Beta Scan Pro 3D"]);
    assert_eq!(title_products("Gamma (Module A, Module B)"), vec!["Gamma"]);
    let names = vec!["Beta Scan".to_string()];
    assert!(covered("Beta Scan Pro 3D", &names));
    assert!(!covered("Beta Scanner", &names));
}

#[test]
fn ac9_every_seed_title_product_is_listed() {
    let seeds = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../seeds");
    let names = incumbents();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&seeds)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    files.sort();
    let mut checked = 0;
    let mut missing = Vec::new();
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        if NOT_COMPETITOR_INVENTORIES.contains(&name.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        let title = text
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .unwrap_or_else(|| panic!("{name}: no title line"));
        for p in title_products(title) {
            checked += 1;
            if !covered(&p, &names) {
                missing.push(format!("{name}: {p}"));
            }
        }
    }
    assert!(checked >= 5, "expected the competitor inventories, checked {checked} products");
    assert!(missing.is_empty(), "seed title products missing from rules/incumbents.txt: {missing:?}");
}
