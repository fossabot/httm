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

use crate::{Config, PathData};

use chrono::{DateTime, Local};
use lscolors::{LsColors, Style};
use number_prefix::NumberPrefix;
use std::{path::Path, time::SystemTime};
use terminal_size::{terminal_size, Height, Width};

pub fn display_exec(
    config: &Config,
    snaps_and_live_set: Vec<Vec<PathData>>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync + 'static>> {
    let output_buffer = if config.opt_raw || config.opt_zeros {
        display_raw(config, snaps_and_live_set)?
    } else {
        display_pretty(config, snaps_and_live_set)?
    };

    Ok(output_buffer)
}

fn display_raw(
    config: &Config,
    snaps_and_live_set: Vec<Vec<PathData>>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync + 'static>> {
    let mut buffer = String::new();

    let delimiter = if config.opt_zeros { '\0' } else { '\n' };

    // so easy!
    snaps_and_live_set.iter().for_each(|version| {
        version.iter().for_each(|pathdata| {
            let display_path = pathdata.path_buf.display();
            buffer += &format!("{}{}", display_path, delimiter);
        });
    });

    Ok(buffer)
}

fn display_pretty(
    config: &Config,
    snaps_and_live_set: Vec<Vec<PathData>>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync + 'static>> {
    let mut write_out_buffer = String::new();

    let (size_padding, fancy_border_string) = calculate_padding(&snaps_and_live_set);

    // now display with all that beautiful padding
    if !config.opt_no_pretty {
        // only print one border to the top -- to write_out_buffer, not pathdata_set_buffer
        write_out_buffer += &format!("{}\n", fancy_border_string);
    }

    snaps_and_live_set
        .iter()
        .enumerate()
        .for_each(|(idx, pathdata_set)| {
            let mut pathdata_set_buffer = String::new();

            pathdata_set.iter().for_each(|pathdata| {
                let pathdata_date = display_date(&pathdata.system_time);
                // 2 space wide padding
                let fixed_width_padding: String = "  ".to_string();

                // tab delimited if "no pretty", no border lines, and no colors
                let (pathdata_size, display_path, display_padding) = if !config.opt_no_pretty {
                    let path_size = format!(
                        "{:>width$}",
                        display_human_size(pathdata),
                        width = size_padding
                    );
                    let display_padding = fixed_width_padding;

                    // paint the live string with ls colors
                    let path = &pathdata.path_buf;
                    let painted_string = if idx == 1 {
                        paint_string(path, &path.to_string_lossy())
                    } else {
                        path.to_string_lossy().to_string()
                    };
                    let display_path = format!("{:<width$}", painted_string, width = size_padding);

                    (path_size, display_path, display_padding)
                } else {
                    let path_size = display_human_size(pathdata);
                    let display_path = pathdata.path_buf.to_string_lossy().into_owned();
                    let display_padding = "\t".to_string();
                    (path_size, display_path, display_padding)
                };

                // displays blanks for phantom values, equaling their dummy lens and dates.
                //
                // see struct PathData for more details as to why we use a dummy instead of
                // a None value here.
                let (display_date, display_size) = if !pathdata.is_phantom {
                    let date = pathdata_date;
                    let size = pathdata_size;
                    (date, size)
                } else {
                    let date: String = (0..pathdata_date.len()).map(|_| " ").collect();
                    let size: String = (0..pathdata_size.len()).map(|_| " ").collect();
                    (date, size)
                };
                pathdata_set_buffer += &format!(
                    "{}{}{}{}\"{}\"\n",
                    display_date, display_padding, display_size, display_padding, display_path
                );
            });
            if !config.opt_no_pretty && !pathdata_set_buffer.is_empty() {
                pathdata_set_buffer += &format!("{}\n", fancy_border_string);
                write_out_buffer += &pathdata_set_buffer;
            } else {
                pathdata_set_buffer.lines().rev().for_each(|line| {
                    write_out_buffer += &format!("{}\n", line);
                });
            }
        });

    Ok(write_out_buffer)
}

fn calculate_padding(snaps_and_live_set: &[Vec<PathData>]) -> (usize, String) {
    let mut size_padding = 0usize;
    let mut fancy_border = 0usize;

    // calculate padding and borders for display later
    snaps_and_live_set.iter().for_each(|ver_set| {
        ver_set.iter().for_each(|pathdata| {
            let display_date = display_date(&pathdata.system_time);
            let display_size = format!(
                "{:>width$}",
                display_human_size(pathdata),
                width = size_padding
            );
            let display_path = &pathdata.path_buf.to_string_lossy();
            let fixed_width_padding_len = 2usize;
            let display_size_len = display_human_size(pathdata).len();
            let formatted_line_len =
                // addition of 2usize is for the two quotation marks we add to the path display 
                display_date.len() + display_size.len() + (2 * fixed_width_padding_len) + display_path.len() + 2usize;

            size_padding = display_size_len.max(size_padding);
            fancy_border = formatted_line_len.max(fancy_border);
        });
    });

    // has to be a more idiomatic way to do this
    // if you know, let me know
    let fancy_border_string: String = if let Some((Width(width), Height(_height))) = terminal_size() {
        if (width as usize) < fancy_border {
            (0..width as usize).map(|_| "─").collect()
        } else {
            (0..fancy_border).map(|_| "─").collect()
        }
    } else {
        (0..fancy_border).map(|_| "─").collect()
    };

    (size_padding, fancy_border_string)
}

fn display_human_size(pathdata: &PathData) -> String {
    let size = pathdata.size as f64;

    match NumberPrefix::binary(size) {
        NumberPrefix::Standalone(bytes) => {
            format!("{} bytes", bytes)
        }
        NumberPrefix::Prefixed(prefix, n) => {
            format!("{:.1} {}B", n, prefix)
        }
    }
}

fn display_date(st: &SystemTime) -> String {
    let dt: DateTime<Local> = st.to_owned().into();
    format!("{}", dt.format("%b %e %Y %H:%M:%S"))
}

pub fn paint_string(path: &Path, file_name: &str) -> String {
    let lscolors = LsColors::from_env().unwrap_or_default();

    if let Some(style) = lscolors.style_for_path(path) {
        let ansi_style = &Style::to_ansi_term_style(style);
        ansi_style.paint(file_name).to_string()
    } else {
        file_name.to_owned()
    }
}
