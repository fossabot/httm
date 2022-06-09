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

use std::{
    fs::read_dir,
    path::{Path, PathBuf},
    time::SystemTime,
};

use fxhash::FxHashMap as HashMap;
use rayon::prelude::*;

use crate::{Config, FilesystemType, HttmError, PathData, SnapPoint, ZFS_SNAPSHOT_DIRECTORY};

pub struct DatasetsForSearch {
    pub immediate_dataset_mount: PathBuf,
    pub datasets_of_interest: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum NativeDatasetType {
    MostImmediate,
    AltReplicated,
}

#[derive(Debug, Clone)]
pub struct SearchBundle {
    pub snapshot_dir: PathBuf,
    pub relative_path: PathBuf,
    pub snapshot_mounts: Option<Vec<PathBuf>>,
}

pub fn get_versions_set(
    config: &Config,
    pathdata: &Vec<PathData>,
) -> Result<[Vec<PathData>; 2], Box<dyn std::error::Error + Send + Sync + 'static>> {
    // prepare for local and replicated backups on alt replicated sets if necessary
    let selected_datasets = if config.opt_alt_replicated {
        vec![
            NativeDatasetType::AltReplicated,
            NativeDatasetType::MostImmediate,
        ]
    } else {
        vec![NativeDatasetType::MostImmediate]
    };

    let all_snap_versions: Vec<PathData> =
        get_all_snap_versions(config, pathdata, &selected_datasets)?;

    // create vec of live copies - unless user doesn't want it!
    let live_versions: Vec<PathData> = if !config.opt_no_live_vers {
        pathdata.to_owned()
    } else {
        Vec::new()
    };

    // check if all files (snap and live) do not exist, if this is true, then user probably messed up
    // and entered a file that never existed (that is, perhaps a wrong file name)?
    if all_snap_versions.is_empty() && live_versions.par_iter().all(|i| i.is_phantom) {
        return Err(HttmError::new(
            "Neither a live copy, nor a snapshot copy of such a file appears to exist, so, umm, 🤷? Please try another file.",
        )
        .into());
    }

    Ok([all_snap_versions, live_versions])
}

fn get_all_snap_versions(
    config: &Config,
    pathdata: &Vec<PathData>,
    selected_datasets: &Vec<NativeDatasetType>,
) -> Result<Vec<PathData>, Box<dyn std::error::Error + Send + Sync + 'static>> {
    // create vec of all local and replicated backups at once
    let all_snap_versions: Vec<PathData> = pathdata
        .par_iter()
        .map(|path_data| {
            selected_datasets
                .par_iter()
                .map(|dataset_type| get_search_bundle(config, path_data, dataset_type))
                .flatten()
        })
        .flatten()
        .flatten()
        .flat_map(|search_bundle| get_versions_per_dataset(config, &search_bundle))
        .flatten()
        .collect();

    Ok(all_snap_versions)
}

pub fn get_search_bundle(
    config: &Config,
    file_pathdata: &PathData,
    requested_dataset_type: &NativeDatasetType,
) -> Result<Vec<SearchBundle>, Box<dyn std::error::Error + Send + Sync + 'static>> {
    // here, we take our file path and get back possibly multiple ZFS dataset mountpoints
    // and our most immediate dataset mount point (which is always the same) for
    // a single file
    //
    // we ask a few questions: has the location been user defined? if not, does
    // the user want all local datasets on the system, including replicated datasets?
    // the most common case is: just use the most immediate dataset mount point as both
    // the dataset of interest and most immediate ZFS dataset
    //
    // why? we need both the dataset of interest and the most immediate dataset because we
    // will compare the most immediate dataset to our our canonical path and the difference
    // between ZFS mount point and the canonical path is the path we will use to search the
    // hidden snapshot dirs
    let dataset_collection: DatasetsForSearch = match &config.snap_point {
        SnapPoint::UserDefined(defined_dirs) => DatasetsForSearch {
            immediate_dataset_mount: defined_dirs.snap_dir.to_owned(),
            datasets_of_interest: vec![defined_dirs.snap_dir.to_owned()],
        },
        SnapPoint::Native(native_datasets) => {
            let immediate_dataset_mount =
                get_immediate_dataset(file_pathdata, &native_datasets.map_of_datasets)?;
            match requested_dataset_type {
                NativeDatasetType::MostImmediate => {
                    // just return the same dataset when in most immediate mode
                    DatasetsForSearch {
                        immediate_dataset_mount: immediate_dataset_mount.clone(),
                        datasets_of_interest: vec![immediate_dataset_mount],
                    }
                }
                NativeDatasetType::AltReplicated => match &native_datasets.map_of_alts {
                    Some(map_of_alts) => match map_of_alts.get(&immediate_dataset_mount) {
                        Some(alternate_mounts) => DatasetsForSearch {
                            immediate_dataset_mount: immediate_dataset_mount.clone(),
                            datasets_of_interest: alternate_mounts.to_owned(),
                        },
                        None => get_alt_replicated_datasets(
                            immediate_dataset_mount.as_path(),
                            &native_datasets.map_of_datasets,
                        )?,
                    },
                    None => get_alt_replicated_datasets(
                        immediate_dataset_mount.as_path(),
                        &native_datasets.map_of_datasets,
                    )?,
                },
            }
        }
    };

    dataset_collection
        .datasets_of_interest
        .par_iter()
        .map(|dataset_of_interest| {
            // building our relative path by removing parent below the snap dir
            //
            // for native searches the prefix is are the dirs below the most immediate dataset
            // for user specified dirs these are specified by the user

            let immediate_dataset_mount = &dataset_collection.immediate_dataset_mount;

            let (snapshot_dir, relative_path, snapshot_mounts) = match &config.snap_point {
                SnapPoint::UserDefined(defined_dirs) => {
                    let snapshot_dir = match &defined_dirs.fstype {
                        FilesystemType::Zfs => dataset_of_interest.join(ZFS_SNAPSHOT_DIRECTORY),
                        FilesystemType::Btrfs => dataset_of_interest.to_path_buf(),
                    };

                    let relative_path = file_pathdata
                        .path_buf
                        .strip_prefix(&defined_dirs.local_dir)?
                        .to_path_buf();

                    let snapshot_mounts = None;

                    (snapshot_dir, relative_path, snapshot_mounts)
                }
                SnapPoint::Native(native_datasets) => {
                    // this prefix removal is why we always need the immediate dataset name, even when we are searching an alternate replicated filesystem

                    // building the snapshot path from our dataset
                    let snapshot_dir = match &native_datasets
                        .map_of_datasets
                        .get(dataset_of_interest)
                    {
                        Some((_, fstype)) => match fstype {
                            FilesystemType::Zfs => dataset_of_interest.join(ZFS_SNAPSHOT_DIRECTORY),
                            FilesystemType::Btrfs => dataset_of_interest.to_path_buf(),
                        },
                        None => dataset_of_interest.join(ZFS_SNAPSHOT_DIRECTORY),
                    };

                    let relative_path = file_pathdata
                        .path_buf
                        .strip_prefix(&immediate_dataset_mount)?
                        .to_path_buf();

                    let snapshot_mounts = match &native_datasets.map_of_snaps {
                        Some(map_of_snaps) => {
                            map_of_snaps.get(dataset_of_interest).map(|t| t.to_owned())
                        }
                        None => None,
                    };

                    (snapshot_dir, relative_path, snapshot_mounts)
                }
            };

            Ok(SearchBundle {
                snapshot_dir,
                relative_path,
                snapshot_mounts,
            })
        })
        .collect()
}

fn get_immediate_dataset(
    pathdata: &PathData,
    mount_collection: &HashMap<PathBuf, (String, FilesystemType)>,
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync + 'static>> {
    // use pathdata as path
    let path = &pathdata.path_buf;

    // prune away most mount points by filtering - parent folder of file must contain relevant dataset
    let potential_mountpoints: Vec<&PathBuf> = mount_collection
        .par_iter()
        .map(|(mount, _dataset)| mount)
        .filter(|mount| path.starts_with(mount))
        .collect();

    // do we have any mount points left? if not print error
    if potential_mountpoints.is_empty() {
        let msg = "Could not identify any qualifying dataset.  Maybe consider specifying manually at SNAP_POINT?";
        return Err(HttmError::new(msg).into());
    };

    // select the best match for us: the longest, as we've already matched on the parent folder
    // so for /usr/bin, we would then prefer /usr/bin to /usr and /
    let best_potential_mountpoint = match potential_mountpoints
        .par_iter()
        .max_by_key(|potential_mountpoint| potential_mountpoint.as_os_str().len())
    {
        Some(some_bpmp) => some_bpmp.to_path_buf(),
        None => {
            let msg = format!(
                "There is no best match for a ZFS dataset to use for path {:?}. Sorry!/Not sorry?)",
                path
            );
            return Err(HttmError::new(&msg).into());
        }
    };

    Ok(best_potential_mountpoint)
}

pub fn get_alt_replicated_datasets(
    immediate_dataset_mount: &Path,
    mount_collection: &HashMap<PathBuf, (String, FilesystemType)>,
) -> Result<DatasetsForSearch, Box<dyn std::error::Error + Send + Sync + 'static>> {
    let immediate_dataset_fs_name = match &mount_collection.get(immediate_dataset_mount) {
        Some((immediate_dataset_fs_name, _)) => immediate_dataset_fs_name.to_string(),
        None => {
            return Err(HttmError::new("httm was unable to detect an alternate replicated mount point.  Perhaps the replicated filesystem is not mounted?").into());
        }
    };

    // find a filesystem that ends with our most local filesystem name
    // but which has a prefix, like a different pool name: rpool might be
    // replicated to tank/rpool
    let mut alt_replicated_mounts: Vec<&PathBuf> = mount_collection
        .par_iter()
        .filter(|(_mount, (fs_name, _fstype))| fs_name != &immediate_dataset_fs_name)
        .filter(|(_mount, (fs_name, _fstype))| {
            fs_name.ends_with(immediate_dataset_fs_name.as_str())
        })
        .map(|(mount, _fs_name)| mount)
        .collect();

    if alt_replicated_mounts.is_empty() {
        // could not find the any replicated mounts
        Err(HttmError::new("httm was unable to detect an alternate replicated mount point.  Perhaps the replicated filesystem is not mounted?").into())
    } else {
        alt_replicated_mounts.sort_unstable_by_key(|path| path.as_os_str().len());
        let datasets_of_interest = alt_replicated_mounts
            .into_iter()
            .map(|alt| alt.to_owned())
            .collect();
        Ok(DatasetsForSearch {
            immediate_dataset_mount: immediate_dataset_mount.to_owned(),
            datasets_of_interest,
        })
    }
}

fn get_versions_per_dataset(
    config: &Config,
    search_bundle: &SearchBundle,
) -> Result<Vec<PathData>, Box<dyn std::error::Error + Send + Sync + 'static>> {
    // get the DirEntry for our snapshot path which will have all our possible
    // snapshots, like so: .zfs/snapshots/<some snap name>/
    //
    // hashmap will then remove duplicates with the same system modify time and size/file len

    let snapshot_dir = search_bundle.snapshot_dir.clone();

    // this is the fallback/non-Linux way of handling without a map_of_snaps
    fn read_dir_for_datasets(
        snapshot_dir: &Path,
        relative_path: &Path,
    ) -> Result<
        HashMap<(SystemTime, u64), PathData>,
        Box<dyn std::error::Error + Send + Sync + 'static>,
    > {
        let unique_versions = read_dir(&snapshot_dir)?
            .flatten()
            .par_bridge()
            .map(|entry| entry.path())
            .map(|path| path.join(relative_path))
            .map(|path| PathData::from(path.as_path()))
            .filter(|pathdata| !pathdata.is_phantom)
            .map(|pathdata| ((pathdata.system_time, pathdata.size), pathdata))
            .collect();

        Ok(unique_versions)
    }

    let unique_versions: HashMap<(SystemTime, u64), PathData> = match &config.snap_point {
        SnapPoint::Native(native_datasets) => match native_datasets.map_of_snaps {
            // Do we have a map_of snaps? If so, get_search_bundle function has already prepared the ones
            // we actually need for this dataset so we can skip the unwrap.
            Some(_) => match search_bundle.snapshot_mounts.as_ref() {
                Some(snap_mounts) => snap_mounts
                    .iter()
                    .map(|path| path.join(&search_bundle.relative_path))
                    .map(|path| PathData::from(path.as_path()))
                    .filter(|pathdata| !pathdata.is_phantom)
                    .map(|pathdata| ((pathdata.system_time, pathdata.size), pathdata))
                    .collect(),
                None => read_dir_for_datasets(&snapshot_dir, &search_bundle.relative_path)?,
            },
            None => read_dir_for_datasets(&snapshot_dir, &search_bundle.relative_path)?,
        },
        SnapPoint::UserDefined(_) => {
            read_dir_for_datasets(&snapshot_dir, &search_bundle.relative_path)?
        }
    };

    let mut vec_pathdata: Vec<PathData> = unique_versions.into_par_iter().map(|(_, v)| v).collect();

    vec_pathdata.par_sort_unstable_by_key(|pathdata| pathdata.system_time);

    Ok(vec_pathdata)
}