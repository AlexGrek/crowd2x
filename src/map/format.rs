//! The map file format: JSON in, JSON out.
//!
//! The runtime [`Map`] and the file are deliberately different shapes, and
//! this module is the only thing that knows both. Serialising the runtime
//! layout directly would put the passability map in the file — a derived
//! structure that could then arrive disagreeing with the terrain it is
//! supposed to describe — and would freeze the in-memory representation,
//! which is chosen for how a simulation reads it and will change.
//!
//! Two decisions the file shape turns on:
//!
//! * **Tiles are stored by name, through a per-file palette.** A [`TerrainId`]
//!   is an index into a table this build happens to have; a later version that
//!   inserts a tile in the middle would silently reinterpret every saved map.
//!   The palette maps the small integers in the rows to names once per file,
//!   and loading resolves those names against the catalogue — so an unknown
//!   tile is a specific, reportable load error instead of a wrong floor.
//! * **A terrain row is a string of comma-separated indices.** JSON has no
//!   compact array form, so a pretty-printed nested array puts every cell of
//!   the map on its own line. As strings, one line of the file is one row of
//!   the map, which is what makes a diff of a map readable. Tiled's JSON
//!   format encodes layers as CSV for the same reason.
//!
//! Rows run bottom to top: row 0 is y 0, because Y increases upward
//! everywhere else in the simulation and a flip that exists only in the file
//! would be a flip somebody eventually forgets.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::{Map, Object, ObjectLayer, Point, Size, TerrainId, BASE};

/// Bumped when a change to the shape below stops older files loading.
pub const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct MapFile {
    version: u32,
    size: SizeFile,
    terrain: TerrainFile,
    /// Keyed by [`ObjectLayer::name`] so the file says which list is which,
    /// rather than depending on the order two arrays happen to be written in.
    objects: BTreeMap<String, Vec<Object>>,
}

#[derive(Serialize, Deserialize)]
struct SizeFile {
    width: i32,
    height: i32,
}

#[derive(Serialize, Deserialize)]
struct TerrainFile {
    /// Tile names, indexed by the numbers in the rows below.
    palette: Vec<String>,
    layers: Vec<LayerFile>,
}

#[derive(Serialize, Deserialize)]
struct LayerFile {
    /// One string per map row, bottom row first, each `width` comma-separated
    /// palette indices.
    rows: Vec<String>,
}

/// Why a map could not be written or read.
#[derive(Debug)]
pub enum MapFormatError {
    Json(serde_json::Error),
    /// Written by a different version of the format.
    UnsupportedVersion(u32),
    InvalidSize { width: i32, height: i32 },
    /// Terrain layers are mandatory; a map with none defines no cells.
    MissingTerrainLayer,
    /// The format allows any number, the runtime supports the base layer.
    UnsupportedLayerCount(usize),
    /// A tile name this build's catalogue does not have.
    UnknownTerrain(String),
    /// A tile id in memory with no catalogue entry, so it has no name to be
    /// written under.
    UnnameableTerrain(u16),
    UnknownObjectLayer(String),
    WrongRowCount { expected: i32, found: usize },
    WrongRowWidth { row: usize, expected: i32, found: usize },
    MalformedRow { row: usize, token: String },
    TileOutOfPalette { row: usize, column: usize, index: u16 },
}

impl fmt::Display for MapFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "not valid map json: {error}"),
            Self::UnsupportedVersion(version) => write!(
                f,
                "map format version {version}, this build reads {FORMAT_VERSION}"
            ),
            Self::InvalidSize { width, height } => {
                write!(f, "{width}x{height} is not a valid map size")
            }
            Self::MissingTerrainLayer => write!(f, "map has no terrain layer"),
            Self::UnsupportedLayerCount(count) => write!(
                f,
                "map has {count} terrain layers, only the base layer is supported yet"
            ),
            Self::UnknownTerrain(name) => write!(f, "unknown terrain tile {name:?}"),
            Self::UnnameableTerrain(id) => {
                write!(f, "terrain id {id} is not in this build's catalogue")
            }
            Self::UnknownObjectLayer(name) => write!(f, "unknown object layer {name:?}"),
            Self::WrongRowCount { expected, found } => {
                write!(f, "expected {expected} terrain rows, found {found}")
            }
            Self::WrongRowWidth {
                row,
                expected,
                found,
            } => write!(f, "row {row} has {found} cells, expected {expected}"),
            Self::MalformedRow { row, token } => {
                write!(f, "row {row} contains {token:?}, expected a palette index")
            }
            Self::TileOutOfPalette { row, column, index } => write!(
                f,
                "row {row} column {column} uses palette index {index}, which the file does not define"
            ),
        }
    }
}

impl std::error::Error for MapFormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for MapFormatError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl Map {
    /// Serialise to pretty-printed JSON — map files live in git, so they are
    /// written to be diffed rather than to be small.
    ///
    /// Fails only on a tile id with no catalogue entry: it has no name to be
    /// stored under, and writing it as anything else would produce a file that
    /// loads as the wrong terrain.
    pub fn to_json(&self) -> Result<String, MapFormatError> {
        // Only the tiles the map actually uses, numbered in the order they are
        // first met. A scan in storage order makes that deterministic, so
        // saving an unchanged map twice produces an identical file.
        let mut palette: Vec<TerrainId> = Vec::new();
        let mut rows: Vec<String> = Vec::with_capacity(self.size.height as usize);
        let tiles = self.base_layer().tiles();

        for y in 0..self.size.height {
            let mut row = String::new();
            for x in 0..self.size.width {
                let tile = tiles[self.size.index_of(Point::new(x, y)).expect("cell is on the map")];
                if tile.terrain().is_none() {
                    return Err(MapFormatError::UnnameableTerrain(tile.0));
                }
                let index = match palette.iter().position(|&known| known == tile) {
                    Some(index) => index,
                    None => {
                        palette.push(tile);
                        palette.len() - 1
                    }
                };
                if x > 0 {
                    row.push(',');
                }
                row.push_str(&index.to_string());
            }
            rows.push(row);
        }

        let file = MapFile {
            version: FORMAT_VERSION,
            size: SizeFile {
                width: self.size.width,
                height: self.size.height,
            },
            terrain: TerrainFile {
                palette: palette.iter().map(|tile| tile.name().to_string()).collect(),
                layers: vec![LayerFile { rows }],
            },
            objects: ObjectLayer::ALL
                .into_iter()
                .map(|layer| (layer.name().to_string(), self.objects(layer).to_vec()))
                .collect(),
        };

        Ok(serde_json::to_string_pretty(&file)?)
    }

    /// Read a map back, rebuilding passability from the terrain that was
    /// stored — so a hand-edited file cannot claim a wall is walkable.
    ///
    /// Every failure is reported rather than repaired: a map that half-loaded
    /// would strand agents in terrain nobody authored.
    pub fn from_json(json: &str) -> Result<Map, MapFormatError> {
        let file: MapFile = serde_json::from_str(json)?;

        if file.version != FORMAT_VERSION {
            return Err(MapFormatError::UnsupportedVersion(file.version));
        }

        let size = Size::try_new(file.size.width, file.size.height).ok_or(
            MapFormatError::InvalidSize {
                width: file.size.width,
                height: file.size.height,
            },
        )?;

        let palette = file
            .terrain
            .palette
            .iter()
            .map(|name| {
                TerrainId::from_name(name).ok_or_else(|| MapFormatError::UnknownTerrain(name.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let layer = match file.terrain.layers.len() {
            0 => return Err(MapFormatError::MissingTerrainLayer),
            1 => &file.terrain.layers[BASE],
            count => return Err(MapFormatError::UnsupportedLayerCount(count)),
        };
        let tiles = decode_layer(layer, size, &palette)?;

        let mut map = Map::from_base_layer(size, tiles);
        for (name, objects) in file.objects {
            let layer = ObjectLayer::from_name(&name)
                .ok_or(MapFormatError::UnknownObjectLayer(name.clone()))?;
            for object in objects {
                map.add_object(layer, object);
            }
        }

        Ok(map)
    }
}

/// One layer's rows into the flat, row-major tile array a [`Map`] holds.
///
/// The dimensions in the header are authoritative: a row of the wrong length
/// is an error rather than something to pad or truncate, because either
/// repair shifts the rest of the map sideways and produces a plausible-looking
/// wrong map instead of a complaint.
fn decode_layer(
    layer: &LayerFile,
    size: Size,
    palette: &[TerrainId],
) -> Result<Vec<TerrainId>, MapFormatError> {
    if layer.rows.len() != size.height as usize {
        return Err(MapFormatError::WrongRowCount {
            expected: size.height,
            found: layer.rows.len(),
        });
    }

    let mut tiles = Vec::with_capacity(size.area());
    for (y, row) in layer.rows.iter().enumerate() {
        let cells: Vec<&str> = row.split(',').map(str::trim).collect();
        if cells.len() != size.width as usize {
            return Err(MapFormatError::WrongRowWidth {
                row: y,
                expected: size.width,
                found: cells.len(),
            });
        }
        for (x, cell) in cells.into_iter().enumerate() {
            let index: u16 = cell.parse().map_err(|_| MapFormatError::MalformedRow {
                row: y,
                token: cell.to_string(),
            })?;
            let tile = palette
                .get(index as usize)
                .ok_or(MapFormatError::TileOutOfPalette {
                    row: y,
                    column: x,
                    index,
                })?;
            tiles.push(*tile);
        }
    }

    Ok(tiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{ObjectKind, FLOOR, VOID, WALL};

    /// A 3x2 room: floor with one wall, a prop and a spawner on it.
    fn sample() -> Map {
        let mut map = Map::new(Size::new(3, 2), FLOOR);
        map.set_terrain(Point::new(1, 1), WALL);
        map.add_object(
            ObjectLayer::Props,
            Object {
                at: Point::new(24, 72),
                kind: ObjectKind::new("bed 1"),
            },
        );
        map.add_object(
            ObjectLayer::Spawners,
            Object {
                at: Point::new(96, 24),
                kind: ObjectKind::new("crowd"),
            },
        );
        map
    }

    fn reload(map: &Map) -> Map {
        Map::from_json(&map.to_json().expect("map is serialisable")).expect("map reloads")
    }

    #[test]
    fn a_map_survives_a_round_trip() {
        let before = sample();
        let after = reload(&before);

        assert_eq!(after.size(), before.size());
        assert_eq!(after.terrain_layers(), before.terrain_layers());
        for point in before.size().points() {
            assert_eq!(after.terrain(point), before.terrain(point), "{point:?}");
        }
        for layer in ObjectLayer::ALL {
            assert_eq!(after.objects(layer), before.objects(layer), "{layer:?}");
        }
    }

    #[test]
    fn loading_rebuilds_passability_from_the_terrain() {
        // Passability is never stored, so this is the check that a loaded map
        // is actually walkable-in the way its terrain says.
        let map = reload(&sample());
        assert!(map.is_passable(Point::new(0, 0)));
        assert!(map.passability().is_blocked(Point::new(1, 1)));
        assert_eq!(map.passability().count_passable(), 5);
    }

    #[test]
    fn saving_twice_produces_the_same_file() {
        let map = sample();
        assert_eq!(map.to_json().unwrap(), reload(&map).to_json().unwrap());
    }

    /// The file shape is the interface to anything outside this program, so a
    /// change to it should have to be made on purpose.
    #[test]
    fn the_file_looks_like_this() {
        let json = sample().to_json().unwrap();
        assert_eq!(
            json,
            r#"{
  "version": 1,
  "size": {
    "width": 3,
    "height": 2
  },
  "terrain": {
    "palette": [
      "floor",
      "wall brown"
    ],
    "layers": [
      {
        "rows": [
          "0,0,0",
          "0,1,0"
        ]
      }
    ]
  },
  "objects": {
    "props": [
      {
        "at": {
          "x": 24,
          "y": 72
        },
        "kind": "bed 1"
      }
    ],
    "spawners": [
      {
        "at": {
          "x": 96,
          "y": 24
        },
        "kind": "crowd"
      }
    ]
  }
}"#
        );
    }

    #[test]
    fn the_palette_holds_only_the_tiles_the_map_uses() {
        let json = Map::new(Size::new(2, 2), VOID).to_json().unwrap();
        assert!(json.contains("\"void\""), "{json}");
        assert!(!json.contains("\"wall\""), "{json}");
    }

    /// Rows are written bottom-up, matching a Y axis that increases upward.
    #[test]
    fn row_zero_is_the_bottom_of_the_map() {
        let mut map = Map::new(Size::new(2, 2), FLOOR);
        map.set_terrain(Point::new(0, 0), WALL);
        let json = map.to_json().unwrap();
        // The wall is at the bottom left and is the first tile met, so it is
        // palette 0, and it appears at the start of the first row written...
        assert!(json.contains("\"0,1\",\n          \"1,1\""), "{json}");
        // ...and it comes back at y = 0, not mirrored to the top.
        assert_eq!(reload(&map).terrain(Point::new(0, 0)), Some(WALL));
    }

    fn load_error(json: &str) -> MapFormatError {
        Map::from_json(json).expect_err("should not have loaded")
    }

    fn sample_json_with(from: &str, to: &str) -> String {
        let json = sample().to_json().unwrap();
        assert!(json.contains(from), "{from} not in {json}");
        json.replace(from, to)
    }

    #[test]
    fn a_file_from_another_format_version_is_refused() {
        let error = load_error(&sample_json_with(r#""version": 1"#, r#""version": 2"#));
        assert!(matches!(error, MapFormatError::UnsupportedVersion(2)), "{error}");
    }

    #[test]
    fn a_tile_this_build_does_not_have_is_named_in_the_error() {
        let error = load_error(&sample_json_with(r#""wall brown""#, r#""force field""#));
        assert!(
            matches!(&error, MapFormatError::UnknownTerrain(name) if name == "force field"),
            "{error}"
        );
    }

    #[test]
    fn rows_that_do_not_match_the_header_are_refused_rather_than_padded() {
        // Truncating or padding would slide the rest of the map sideways and
        // load a wrong map that looks right.
        let short = load_error(&sample_json_with(r#""0,0,0""#, r#""0,0""#));
        assert!(
            matches!(short, MapFormatError::WrongRowWidth { row: 0, expected: 3, found: 2 }),
            "{short}"
        );

        let missing = load_error(&sample_json_with("\"0,0,0\",\n          ", ""));
        assert!(
            matches!(missing, MapFormatError::WrongRowCount { expected: 2, found: 1 }),
            "{missing}"
        );
    }

    #[test]
    fn a_row_can_only_hold_palette_indices() {
        let malformed = load_error(&sample_json_with(r#""0,1,0""#, r#""0,wall,0""#));
        assert!(
            matches!(&malformed, MapFormatError::MalformedRow { row: 1, token } if token == "wall"),
            "{malformed}"
        );

        let unknown = load_error(&sample_json_with(r#""0,1,0""#, r#""0,9,0""#));
        assert!(
            matches!(
                unknown,
                MapFormatError::TileOutOfPalette { row: 1, column: 1, index: 9 }
            ),
            "{unknown}"
        );
    }

    #[test]
    fn a_map_must_have_exactly_one_terrain_layer_for_now() {
        let none = load_error(&sample_json_with(
            "\"layers\": [\n      {\n        \"rows\": [\n          \"0,0,0\",\n          \"0,1,0\"\n        ]\n      }\n    ]",
            "\"layers\": []",
        ));
        assert!(matches!(none, MapFormatError::MissingTerrainLayer), "{none}");

        let two = load_error(&sample_json_with(
            "{\n        \"rows\": [\n          \"0,0,0\",\n          \"0,1,0\"\n        ]\n      }\n    ]",
            "{\n        \"rows\": [\n          \"0,0,0\",\n          \"0,1,0\"\n        ]\n      },\n      {\n        \"rows\": [\n          \"0,0,0\",\n          \"0,1,0\"\n        ]\n      }\n    ]",
        ));
        assert!(matches!(two, MapFormatError::UnsupportedLayerCount(2)), "{two}");
    }

    #[test]
    fn a_size_no_map_can_have_is_an_error_not_a_panic() {
        let error = load_error(&sample_json_with(r#""width": 3"#, r#""width": 0"#));
        assert!(
            matches!(error, MapFormatError::InvalidSize { width: 0, height: 2 }),
            "{error}"
        );
    }

    #[test]
    fn an_object_may_sit_outside_the_terrain() {
        // Objects are free placement, so a prop past the last floor tile is a
        // placement decision and must survive a round trip.
        let map = Map::from_json(&sample_json_with(r#""x": 96"#, r#""x": -400"#)).unwrap();
        assert_eq!(map.objects(ObjectLayer::Spawners)[0].at, Point::new(-400, 24));
    }

    #[test]
    fn an_object_layer_this_build_does_not_have_is_refused() {
        // Dropping it would quietly delete somebody's work on the next save.
        let error = load_error(&sample_json_with(r#""props""#, r#""hazards""#));
        assert!(
            matches!(&error, MapFormatError::UnknownObjectLayer(name) if name == "hazards"),
            "{error}"
        );
    }

    #[test]
    fn junk_is_reported_as_json_rather_than_panicking() {
        assert!(matches!(load_error("{"), MapFormatError::Json(_)));
    }
}
