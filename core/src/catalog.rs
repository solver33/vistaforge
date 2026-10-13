//! The block catalog exported by the Vistaforge exporter mod (`vistaforge/catalog.json`).
//!
//! The generator only emits block states that exist in the user's own game, so every state goes
//! through [`Catalog::state`], which checks names and values and fills in defaults.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// One concrete block state, e.g. `conquest:grass_block_layer[layers=3,waterlogged=false]`.
/// Properties are complete (every property of the block) and sorted by name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockState {
    pub name: String,
    pub props: Vec<(String, String)>,
}

impl BlockState {
    pub fn air() -> Self {
        Self {
            name: "minecraft:air".into(),
            props: Vec::new(),
        }
    }

    pub fn is_air(&self) -> bool {
        self.name == "minecraft:air"
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.props
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

impl fmt::Display for BlockState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)?;
        if !self.props.is_empty() {
            f.write_str("[")?;
            for (i, (k, v)) in self.props.iter().enumerate() {
                if i > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{k}={v}")?;
            }
            f.write_str("]")?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct RawCatalog {
    format: u32,
    #[serde(default)]
    mods: BTreeMap<String, Option<String>>,
    #[serde(default)]
    classes: HashMap<String, Option<String>>,
    blocks: HashMap<String, RawBlock>,
}

#[derive(Debug, Deserialize)]
struct RawBlock {
    class: String,
    #[serde(default)]
    properties: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    default: BTreeMap<String, String>,
    #[serde(default)]
    opaque: Option<bool>,
    #[serde(default)]
    top: Option<RawFace>,
    #[serde(default)]
    side: Option<RawFace>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawFace {
    rgb: String,
    #[serde(default)]
    tint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BlockInfo {
    /// Runtime class name, e.g. `com.conquestrefabricated.content.blocks.block.Layer`.
    pub class: String,
    pub properties: BTreeMap<String, Vec<String>>,
    pub defaults: BTreeMap<String, String>,
    pub opaque: bool,
    /// Average top colour with the default tint applied, 0xRRGGBB.
    pub top_rgb: Option<u32>,
    pub side_rgb: Option<u32>,
}

pub struct Catalog {
    pub mods: BTreeMap<String, Option<String>>,
    blocks: HashMap<String, BlockInfo>,
    classes: HashMap<String, Option<String>>,
}

impl Catalog {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_json(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let raw: RawCatalog = serde_json::from_str(text)?;
        if raw.format != 1 {
            bail!("unsupported catalog format {}", raw.format);
        }
        let blocks = raw
            .blocks
            .into_iter()
            .map(|(id, b)| {
                let info = BlockInfo {
                    class: b.class,
                    properties: b.properties,
                    defaults: b.default,
                    opaque: b.opaque.unwrap_or(true),
                    top_rgb: b.top.as_ref().and_then(face_rgb),
                    side_rgb: b.side.as_ref().and_then(face_rgb),
                };
                (id, info)
            })
            .collect();
        Ok(Self {
            mods: raw.mods,
            blocks,
            classes: raw.classes,
        })
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn block(&self, id: &str) -> Option<&BlockInfo> {
        self.blocks.get(id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.blocks.contains_key(id)
    }

    /// Short class name of a block, e.g. `BranchLarge`.
    pub fn class_of(&self, id: &str) -> Option<&str> {
        self.blocks
            .get(id)
            .map(|b| b.class.rsplit(['.', '$']).next().unwrap_or(&b.class))
    }

    /// True when the block's class, or any superclass, has the given short name.
    pub fn is_a(&self, id: &str, short_class: &str) -> bool {
        let Some(info) = self.blocks.get(id) else {
            return false;
        };
        let mut current = Some(info.class.as_str());
        while let Some(cls) = current {
            if cls.rsplit(['.', '$']).next() == Some(short_class) {
                return true;
            }
            current = self.classes.get(cls).and_then(|p| p.as_deref());
        }
        false
    }

    /// Builds a complete, validated state. Unlisted properties take the block's default value.
    pub fn state(&self, id: &str, props: &[(&str, &str)]) -> Result<BlockState> {
        let info = self
            .blocks
            .get(id)
            .with_context(|| format!("block {id} is not in the catalog"))?;
        for (key, value) in props {
            let allowed = info
                .properties
                .get(*key)
                .with_context(|| format!("block {id} has no property {key}"))?;
            if !allowed.iter().any(|v| v == value) {
                bail!("{id}: {key}={value} is not one of {allowed:?}");
            }
        }
        let props = info
            .properties
            .keys()
            .map(|key| {
                let value = props
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.to_string())
                    .or_else(|| info.defaults.get(key).cloned())
                    .unwrap_or_else(|| info.properties[key][0].clone());
                (key.clone(), value)
            })
            .collect();
        Ok(BlockState {
            name: id.to_string(),
            props,
        })
    }

    /// Like [`Catalog::state`] with a block's default state.
    pub fn default_state(&self, id: &str) -> Result<BlockState> {
        self.state(id, &[])
    }
}

fn face_rgb(face: &RawFace) -> Option<u32> {
    let rgb = u32::from_str_radix(&face.rgb, 16).ok()?;
    match face
        .tint
        .as_deref()
        .and_then(|t| u32::from_str_radix(t, 16).ok())
    {
        Some(tint) => {
            let ch = |c: u32, shift: u32| {
                (((c >> shift) & 0xFF) * ((tint >> shift) & 0xFF) / 255) << shift
            };
            Some(ch(rgb, 16) | ch(rgb, 8) | ch(rgb, 0))
        }
        None => Some(rgb),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SAMPLE: &str = r#"{
        "format": 1, "mods": {"minecraft": "1.20.1"},
        "classes": {"x.Layer": "x.WaterloggedShape", "x.WaterloggedShape": "net.minecraft.class_2248", "net.minecraft.class_2248": "java.lang.Object"},
        "blocks": {
            "minecraft:air": {"class": "net.minecraft.class_2189", "states": 1},
            "minecraft:stone": {"class": "net.minecraft.class_2248", "states": 1, "top": {"rgb": "887c6d", "coverage": 1.0}},
            "test:layer": {"class": "x.Layer", "states": 16,
                "properties": {"layers": ["1","2","3","4","5","6","7","8"], "waterlogged": ["true","false"]},
                "default": {"layers": "1", "waterlogged": "false"}}
        }
    }"#;

    #[test]
    fn validates_and_fills_defaults() {
        let c = Catalog::from_json(SAMPLE).unwrap();
        let s = c.state("test:layer", &[("layers", "3")]).unwrap();
        assert_eq!(s.to_string(), "test:layer[layers=3,waterlogged=false]");
        assert!(c.state("test:layer", &[("layers", "9")]).is_err());
        assert!(c.state("test:layer", &[("colour", "red")]).is_err());
        assert!(c.state("test:missing", &[]).is_err());
        assert!(c.is_a("test:layer", "Layer"));
        assert!(c.is_a("test:layer", "WaterloggedShape"));
        assert!(!c.is_a("minecraft:stone", "Layer"));
        assert_eq!(c.block("minecraft:stone").unwrap().top_rgb, Some(0x887c6d));
    }
}
