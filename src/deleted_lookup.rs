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

use std::{ffi::OsString, fs::read_dir, path::Path};

use fxhash::FxHashMap as HashMap;
use itertools::Itertools;

use crate::versions_lookup::{get_search_bundle, NativeDatasetType, SearchBundle};
use crate::{BasicDirEntryInfo, Config, PathData, SnapPoint};

pub fn get_unique_deleted(
    config: &Config,
    requested_dir: &Path,
) -> Result<Vec<BasicDirEntryInfo>, Box<dyn std::error::Error + Send + Sync + 'static>> {
    // prepare for local and replicated backups on alt replicated sets if necessary
    let selected_datasets = if config.opt_alt_replicated {
        vec![
            NativeDatasetType::AltReplicated,
            NativeDatasetType::MostImmediate,
        ]
    } else {
        vec![NativeDatasetType::MostImmediate]
    };

    // we always need a requesting dir because we are comparing the files in the
    // requesting dir to those of their relative dirs on snapshots
    let requested_dir_pathdata = PathData::from(requested_dir);

    // create vec of all local and replicated backups at once
    //
    // we need to make certain that what we return from possibly multiple datasets are unique
    // as these will be the filenames that populate our interactive views, so deduplicate
    // by filename and latest file version here
    let unique_deleted: Vec<BasicDirEntryInfo> = vec![&requested_dir_pathdata]
        .iter()
        .flat_map(|pathdata| {
            selected_datasets
                .iter()
                .flat_map(|dataset_type| get_search_bundle(config, pathdata, dataset_type))
        })
        .flatten()
        .flat_map(|search_bundle| {
            get_deleted_per_dataset(config, &requested_dir_pathdata.path_buf, &search_bundle)
        })
        .flatten()
        .filter_map(
            |basic_dir_entry_info| match basic_dir_entry_info.path.symlink_metadata() {
                Ok(md) => Some((md, basic_dir_entry_info)),
                Err(_) => None,
            },
        )
        .filter_map(|(md, basic_dir_entry_info)| match md.modified() {
            Ok(modify_time) => Some((modify_time, basic_dir_entry_info)),
            Err(_) => None,
        })
        // this part right here functions like a hashmap, separate into buckets/groups
        // by file name, then return the oldest deleted dir entry, or max by its modify time
        // why? because this might be a folder that has been deleted and we need some policy
        // to give later functions an idea about which folder to choose when we want too look
        // behind deleted dirs, here we just choose latest in time
        .into_group_map_by(|(_modify_time, basic_dir_entry_info)| {
            basic_dir_entry_info.file_name.clone()
        })
        .iter()
        .filter_map(|(_key, group)| {
            group
                .iter()
                .max_by_key(|(modify_time, _basic_dir_entry_info)| *modify_time)
        })
        .map(|(_modify_time, basic_dir_entry_info)| basic_dir_entry_info)
        .cloned()
        .collect();

    Ok(unique_deleted)
}

pub fn get_deleted_per_dataset(
    config: &Config,
    requested_dir: &Path,
    search_bundle: &SearchBundle,
) -> Result<Vec<BasicDirEntryInfo>, Box<dyn std::error::Error + Send + Sync + 'static>> {
    // get all local entries we need to compare against these to know
    // what is a deleted file
    // create a collection of local unique file names
    let unique_local_filenames: HashMap<OsString, BasicDirEntryInfo> = read_dir(&requested_dir)?
        .flatten()
        .map(|dir_entry| {
            (
                dir_entry.file_name(),
                BasicDirEntryInfo {
                    file_name: dir_entry.file_name(),
                    path: dir_entry.path(),
                    file_type: dir_entry.file_type().ok(),
                },
            )
        })
        .collect();

    // now create a collection of file names in the snap_dirs
    // create a list of unique filenames on snaps

    fn read_dir_for_snap_filenames(
        snapshot_dir: &Path,
        relative_path: &Path,
    ) -> Result<
        HashMap<OsString, BasicDirEntryInfo>,
        Box<dyn std::error::Error + Send + Sync + 'static>,
    > {
        let unique_snap_filenames = read_dir(&snapshot_dir)?
            .flatten()
            .map(|entry| entry.path())
            .map(|path| path.join(relative_path))
            .flat_map(|path| read_dir(&path))
            .flatten()
            .flatten()
            .map(|dir_entry| {
                (
                    dir_entry.file_name(),
                    BasicDirEntryInfo {
                        file_name: dir_entry.file_name(),
                        path: dir_entry.path(),
                        file_type: dir_entry.file_type().ok(),
                    },
                )
            })
            .collect();

        Ok(unique_snap_filenames)
    }

    let unique_snap_filenames: HashMap<OsString, BasicDirEntryInfo> = match &config.snap_point {
        SnapPoint::Native(native_datasets) => match native_datasets.map_of_snaps {
            // Do we have a map_of snaps? If so, get_search_bundle function has already prepared the ones
            // we actually need for this dataset so we can skip the unwrap.
            Some(_) => match search_bundle.snapshot_mounts.as_ref() {
                Some(snap_mounts) => snap_mounts
                    .iter()
                    .map(|path| path.join(&search_bundle.relative_path))
                    .flat_map(|path| read_dir(&path))
                    .flatten()
                    .flatten()
                    .map(|dir_entry| {
                        (
                            dir_entry.file_name(),
                            BasicDirEntryInfo {
                                file_name: dir_entry.file_name(),
                                path: dir_entry.path(),
                                file_type: dir_entry.file_type().ok(),
                            },
                        )
                    })
                    .collect(),
                None => read_dir_for_snap_filenames(
                    &search_bundle.snapshot_dir,
                    &search_bundle.relative_path,
                )?,
            },
            None => read_dir_for_snap_filenames(
                &search_bundle.snapshot_dir,
                &search_bundle.relative_path,
            )?,
        },
        SnapPoint::UserDefined(_) => {
            read_dir_for_snap_filenames(&search_bundle.snapshot_dir, &search_bundle.relative_path)?
        }
    };

    // compare local filenames to all unique snap filenames - none values are unique here
    let all_deleted_versions: Vec<BasicDirEntryInfo> = unique_snap_filenames
        .into_iter()
        .filter(|(file_name, _)| unique_local_filenames.get(file_name).is_none())
        .map(|(_file_name, basic_dir_entry_info)| basic_dir_entry_info)
        .collect();

    Ok(all_deleted_versions)
}