//       ___           ___           ___           ___
//      /\__\         /\  \         /\  \         /\__\
//     /:/  /         \:\  \        \:\  \       /::|  |
//    /:/__/           \:\  \        \:\  \     /:|:|  |
//   /::\  \ ___       /::\  \       /::\  \   /:/|:|__|__
//  /:/\:\  /\__\     /:/\:\__\     /:/\:\__\ /:/ |::::\__\
//  \/__\:\/:/  /    /:/  \/__/    /:/  \/__/ \/__/~~/:/  /
//       \::/  /    /:/  /        /:/  /            /:/  /
//       /:/  /     \/__/         \/__/            /:/  /
//      /:/  /                                    /:/  /
//      \/__/                                     \/__/
//
// (c) Robert Swinford <robert.swinford<...at...>gmail.com>
//
// For the full copyright and license information, please view the LICENSE file
// that was distributed with this source code.

use std::{ffi::OsString, path::Path, path::PathBuf};

use crate::utility::{get_fs_type_from_hidden_dir, HttmError};
use crate::{HttmResult, MapOfAliases, RemotePathAndFsType};

pub fn parse_aliases(
    raw_local_dir: &Option<OsString>,
    raw_snap_dir: &Option<OsString>,
    pwd: &Path,
    opt_input_aliases: &Option<Vec<String>>,
) -> HttmResult<MapOfAliases> {
    // user defined dir exists?: check that path contains the hidden snapshot directory
    let snap_point = raw_snap_dir.as_ref().map(|value| {
        let snap_dir = PathBuf::from(value);

        // local relative dir can be set at cmdline or as an env var,
        // but defaults to current working directory if empty
        let local_dir = match raw_local_dir {
            Some(value) => PathBuf::from(value),
            None => pwd.to_path_buf(),
        };

        (snap_dir, local_dir)
    });

    let mut aliases_iter: Vec<(PathBuf, PathBuf)> = match opt_input_aliases {
        Some(input_aliases) => {
            let res: Option<Vec<(PathBuf, PathBuf)>> = input_aliases
                .iter()
                .map(|alias| {
                    alias
                        .split_once(':')
                        .map(|(first, rest)| (PathBuf::from(first), PathBuf::from(rest)))
                })
                .collect();

            match res.ok_or_else(|| {
                HttmError::new(
                    "Must use specified delimiter (':') between aliases for MAP_ALIASES.",
                )
            }) {
                Ok(res) => res,
                Err(err) => return Err(err.into()),
            }
        }
        None => Vec::new(),
    };

    if let Some(value) = snap_point {
        aliases_iter.push(value)
    }

    let map_of_aliases = aliases_iter
        .into_iter()
        .flat_map(|(local_dir, snap_dir)| {
            if !local_dir.exists() || !snap_dir.exists() {
                [local_dir, snap_dir]
                    .into_iter()
                    .filter(|dir| !dir.exists())
                    .for_each(|dir| {
                        eprintln!(
                        "Warning: An alias path specified does not exist, or is not mounted: {:?}",
                        dir
                    )
                    });
                None
            } else {
                Some((local_dir, snap_dir))
            }
        })
        .flat_map(|(local_dir, remote_dir)| {
            get_fs_type_from_hidden_dir(&remote_dir)
                .ok()
                .map(|fs_type| {
                    (
                        local_dir,
                        RemotePathAndFsType {
                            remote_dir,
                            fs_type,
                        },
                    )
                })
        })
        .collect();

    Ok(map_of_aliases)
}
