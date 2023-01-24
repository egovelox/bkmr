use camino::Utf8Path;
use std::fs::create_dir_all;
use std::io::Write;
use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use diesel::result::DatabaseErrorKind;
use diesel::result::Error::DatabaseError;

use log::{debug, error, info};
use stdext::function_name;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

use bkmr::bms::Bookmarks;
use bkmr::dal::Dal;
use bkmr::environment::CONFIG;
use bkmr::fzf::fzf_process;
use bkmr::helper::{ensure_int_vector, init_db};
use bkmr::load_url_details;
use bkmr::models::NewBookmark;
use bkmr::process::{delete_bms, edit_bms, process, show_bms};
use bkmr::tag::Tags;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
/// A bookmark manager for the terminal
struct Cli {
    /// Optional name to operate on
    name: Option<String>,

    /// Sets a custom config file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Turn debugging information on
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Searches Bookmarks
    Search {
        /// FTS query (full text search)
        fts_query: Option<String>,

        #[arg(
            short = 'e',
            long = "exact",
            help = "match exact, comma separated list"
        )]
        tags_exact: Option<String>,

        #[arg(short = 't', long = "tags", help = "match all, comma separated list")]
        tags_all: Option<String>,

        #[arg(
            short = 'T',
            long = "Tags",
            help = "not match all, comma separated list"
        )]
        tags_all_not: Option<String>,

        #[arg(short = 'n', long = "ntags", help = "match any, comma separated list")]
        tags_any: Option<String>,

        #[arg(
            short = 'N',
            long = "Ntags",
            help = "not match any, comma separated list"
        )]
        tags_any_not: Option<String>,

        #[arg(long = "prefix", help = "tags to prefix the tags option")]
        tags_prefix: Option<String>,

        #[arg(short = 'o', long = "descending", help = "order by age, descending")]
        order_desc: bool,

        #[arg(short = 'O', long = "ascending", help = "order by age, ascending")]
        order_asc: bool,

        #[arg(long = "np", help = "no prompt")]
        non_interactive: bool,

        #[arg(long = "fzf", help = "use fuzzy finder")]
        is_fuzzy: bool,
    },
    /// Open/launch bookmarks
    Open {
        /// list of ids, separated by comma, no blanks
        ids: String,
    },
    /// add a bookmark
    Add {
        url: String,
        /// list of tags, separated by comma, no blanks in between
        tags: Option<String>,
        #[arg(long = "title", help = "title")]
        title: Option<String>,
        #[arg(short = 'd', long = "description", help = "title")]
        desc: Option<String>,
        #[arg(long = "no-web", help = "do not fetch URL data")]
        no_web: bool,
        #[arg(short = 'e', long = "edit", help = "edit the bookmark while adding")]
        edit: bool,
    },
    /// Delete bookmarks
    Delete {
        /// list of ids, separated by comma, no blanks
        ids: String,
    },
    /// Update bookmarks
    Update {
        /// list of ids, separated by comma, no blanks
        ids: String,
        #[arg(short = 't', long = "tags", help = "add tags to taglist")]
        tags: Option<String>,
        #[arg(short = 'n', long = "ntags", help = "remove tags from taglist")]
        tags_not: Option<String>,
        #[arg(short = 'f', long = "force", help = "overwrite taglist with tags")]
        force: bool,
    },
    /// Edit bookmarks
    Edit {
        /// Edit bookmarks, list of ids, separated by comma, no blanks
        ids: String,
    },
    /// Show Bookmarks (list of ids, separated by comma, no blanks)
    Show { ids: String },
    /// tag for which related tags should be shown. No input: all tags are printed
    Tags {
        /// tag for which related tags should be shown. No input: all tags are shown
        tag: Option<String>,
    },
    /// Initialize bookmark database
    CreateDb {
        /// pathname to database file
        path: String,
    },
    #[command(hide = true)]
    Xxx {
        /// list of ids, separated by comma, no blanks
        ids: String,
        #[arg(short = 't', long = "tags", help = "add tags to taglist")]
        tags: Option<String>,
    },
}

fn main() {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    let cli = Cli::parse();
    // Note, only flags can have multiple occurrences
    match cli.debug {
        0 => {
            let _ = env_logger::builder()
                .filter_level(log::LevelFilter::Warn)
                .try_init();
        }
        1 => {
            let _ = env_logger::builder()
                .filter_level(log::LevelFilter::Info)
                .filter_module("skim", log::LevelFilter::Info)
                .filter_module("tuikit", log::LevelFilter::Info)
                .filter_module("html5ever", log::LevelFilter::Info)
                .filter_module("reqwest", log::LevelFilter::Info)
                .filter_module("mio", log::LevelFilter::Info)
                .filter_module("want", log::LevelFilter::Info)
                .try_init();
            info!("Debug mode: info");
        }
        2 => {
            let _ = env_logger::builder()
                .filter_level(log::LevelFilter::max())
                .filter_module("skim", log::LevelFilter::Info)
                .filter_module("tuikit", log::LevelFilter::Info)
                .filter_module("html5ever", log::LevelFilter::Info)
                .filter_module("reqwest", log::LevelFilter::Info)
                .filter_module("mio", log::LevelFilter::Info)
                .filter_module("want", log::LevelFilter::Info)
                .try_init();
            debug!("Debug mode: debug");
        }
        _ => eprintln!("Don't be crazy"),
    }

    match &cli.command {
        Some(Commands::Search {
            fts_query,
            tags_exact,
            tags_all,
            tags_all_not,
            tags_any,
            tags_any_not,
            tags_prefix,
            order_desc,
            order_asc,
            non_interactive,
            is_fuzzy,
        }) => {
            let mut _tags_all = String::from("");
            if tags_prefix.is_some() {
                if tags_all.is_none() {
                    _tags_all = tags_prefix.clone().unwrap();
                } else {
                    _tags_all = format!(
                        "{},{}",
                        tags_all.clone().unwrap(),
                        tags_prefix.clone().unwrap()
                    );
                }
            } else {
                _tags_all = tags_all.clone().unwrap_or_default();
            }
            debug!("({}:{}) tags: {:?}", function_name!(), line!(), _tags_all);

            let fts_query = fts_query.clone().unwrap_or("".to_string());

            let mut bms = Bookmarks::new(fts_query);
            bms.filter(
                Some(_tags_all),
                tags_any.clone(),
                tags_all_not.clone(),
                tags_any_not.clone(),
                tags_exact.clone(),
            );

            if *order_desc {
                debug!(
                    "({}:{}) order_desc {:?}",
                    function_name!(),
                    line!(),
                    order_desc
                );
                bms.bms.sort_by_key(|bm| bm.last_update_ts);
                bms.bms.reverse();
            } else if *order_asc {
                debug!(
                    "({}:{}) order_asc {:?}",
                    function_name!(),
                    line!(),
                    order_asc
                );
                bms.bms.sort_by_key(|bm| bm.last_update_ts);
            } else {
                debug!("({}:{}) order_by_metadata", function_name!(), line!());
                bms.bms.sort_by_key(|bm| bm.metadata.to_lowercase())
            }

            if *is_fuzzy {
                fzf_process(&bms.bms);
                return ();
            }
            debug!("({}:{})\n{:#?}\n", function_name!(), line!(), bms.bms);
            show_bms(&bms.bms);

            if *non_interactive {
                debug!("Non Interactive. Exiting");
                // process(bms);
            } else {
                println!("Found {} bookmarks", bms.bms.len());
                stdout
                    .set_color(ColorSpec::new().set_fg(Some(Color::Green)))
                    .unwrap();
                writeln!(&mut stdout, "Selection: ").unwrap();
                stdout.reset().unwrap();
                process(&bms.bms);
            }
        }
        Some(Commands::Open { ids }) => {
            let mut dal = Dal::new(CONFIG.db_url.clone());
            let ids: Vec<String> = ids.split(',').map(|s| s.to_owned()).collect();
            let ids = ensure_int_vector(&ids);
            if ids.is_none() {
                error!(
                    "({}:{}) Invalid input, only numbers allowed {:?}",
                    function_name!(),
                    line!(),
                    ids
                );
                return;
            }

            for id in ids.unwrap() {
                let bm = dal.get_bookmark_by_id(id);
                match bm {
                    Ok(bm) => {
                        debug!("({}:{}) Opening {:?}", function_name!(), line!(), bm);
                        open::that(bm.URL).unwrap();
                    }
                    Err(_) => {
                        error!(
                            "({}:{}) Bookmark with id {} not found",
                            function_name!(),
                            line!(),
                            id
                        );
                    }
                }
            }
        }
        Some(Commands::Add {
            url,
            tags,
            title,
            desc,
            no_web,
            edit,
        }) => {
            let mut dal = Dal::new(CONFIG.db_url.clone());
            debug!(
                "({}:{}) Add {:?}, {:?}, {:?}, {:?}, {:?}, {:?}",
                function_name!(),
                line!(),
                url,
                tags,
                title,
                desc,
                no_web,
                edit,
            );

            let (_title, _description, _keywords) = if !*no_web {
                let result = load_url_details(&url);
                match result {
                    Ok(details) => {
                        debug!(
                            "({}:{}) Fetched URL: {:?}",
                            function_name!(),
                            line!(),
                            details
                        );
                        (
                            details.0.unwrap_or("".to_string()),
                            details.1.unwrap_or("".to_string()),
                            details.2.unwrap_or("".to_string()),
                        )
                    }
                    Err(e) => {
                        debug!("Cannot enrich URL details from web: {:?}", e);
                        eprintln!("Cannot enrich URL data from web.");
                        ("".to_string(), "".to_string(), "".to_string())
                    }
                }
            } else {
                ("".to_string(), "".to_string(), "".to_string())
            };

            let title = title.to_owned().unwrap_or(_title);
            let description = desc.to_owned().unwrap_or(_description);
            debug!(
                "({}:{}) title: {:?}, description: {:?}",
                function_name!(),
                line!(),
                title,
                description
            );

            match dal.insert_bookmark(NewBookmark {
                URL: url.to_string(),
                metadata: title,
                tags: Tags::create_normalized_tag_string(tags.to_owned()),
                desc: description,
                flags: 0,
            }) {
                Ok(bms) => {
                    if *edit {
                        edit_bms(vec![1], bms.clone()).unwrap_or_else(|e| {
                            error!(
                                "({}:{}) Error editing bookmark: {:?}",
                                function_name!(),
                                line!(),
                                e
                            );
                        });
                    }
                    println!("Added bookmark: {:?}", bms[0].id);
                    show_bms(&bms)
                }
                Err(e) => {
                    if let DatabaseError(DatabaseErrorKind::UniqueViolation, _) = e {
                        eprintln!("Bookmark already exists: {}", url);
                    } else {
                        error!(
                            "({}:{}) Error adding bookmark: {:?}",
                            function_name!(),
                            line!(),
                            e
                        );
                    }
                }
            }
        }
        Some(Commands::Delete { ids }) => {
            let ids = ensure_int_vector(&ids.split(',').map(|s| s.to_owned()).collect());
            if ids.is_none() {
                eprintln!(
                    "({}:{}) Invalid input, only numbers allowed {:?}",
                    function_name!(),
                    line!(),
                    ids
                );
                process::exit(1);
            }
            let bms = Bookmarks::new("".to_string()); // load all bms
            delete_bms(ids.clone().unwrap(), bms.bms.clone()).unwrap_or_else(|e| {
                eprintln!(
                    "Error ({}:{}) Deleting Bookmarks: {:?}",
                    function_name!(),
                    line!(),
                    e
                );
                process::exit(1);
            });
        }
        Some(Commands::Update {
            ids,
            tags,
            tags_not,
            force,
        }) => {
            if *force && (tags.is_none() || tags_not.is_some()) {
                eprintln!(
                    "({}:{}) Force update requires tags but no ntags.",
                    function_name!(),
                    line!()
                );
                process::exit(1);
            }
            let ids = ensure_int_vector(&ids.split(',').map(|s| s.to_owned()).collect());
            if ids.is_none() {
                eprintln!(
                    "({}:{}) Invalid input, only numbers allowed {:?}",
                    function_name!(),
                    line!(),
                    ids
                );
                process::exit(1);
            }
            let tags = Tags::normalize_tag_string(tags.clone());
            let tags_not = Tags::normalize_tag_string(tags_not.clone());
            println!("Update {:?}, {:?}, {:?}, {:?}", ids, tags, tags_not, force);
            bkmr::update_bookmarks(ids.unwrap(), tags, tags_not, *force);
        }
        Some(Commands::Edit { ids }) => {
            let ids = ensure_int_vector(&ids.split(',').map(|s| s.to_owned()).collect());
            if ids.is_none() {
                eprintln!(
                    "({}:{}) Invalid input, only numbers allowed {:?}",
                    function_name!(),
                    line!(),
                    ids
                );
                process::exit(1);
            }
            let bms = Bookmarks::new("".to_string()); // load all bms
            edit_bms(ids.unwrap(), bms.bms.clone()).unwrap_or_else(|e| {
                eprintln!(
                    "Error ({}:{}) Editing Bookmarks: {:?}",
                    function_name!(),
                    line!(),
                    e
                );
                process::exit(1);
            });
        }
        Some(Commands::Show { ids }) => {
            let mut dal = Dal::new(CONFIG.db_url.clone());
            let ids = ensure_int_vector(&ids.split(',').map(|s| s.to_owned()).collect());
            if ids.is_none() {
                eprintln!(
                    "({}:{}) Invalid input, only numbers allowed {:?}",
                    function_name!(),
                    line!(),
                    ids
                );
                process::exit(1);
            }
            let mut bms = vec![];
            for id in ids.unwrap() {
                let bm = dal.get_bookmark_by_id(id);
                match bm {
                    Ok(bm) => {
                        debug!("({}:{}) {:?}", function_name!(), line!(), bm);
                        bms.push(bm);
                    }
                    Err(_) => {
                        eprintln!("Bookmark with id {} not found", id);
                    }
                }
            }
            show_bms(&bms);
        }
        Some(Commands::Tags { tag }) => {
            let mut dal = Dal::new(CONFIG.db_url.clone());
            let tags = match tag {
                Some(tag) => dal.get_related_tags(tag),
                None => dal.get_all_tags(),
            };
            match tags {
                Ok(tags) => {
                    for tag in tags {
                        println!("{}: {}", tag.n, tag.tag);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "Error ({}:{}) Getting all tags: {:?}",
                        function_name!(),
                        line!(),
                        e
                    );
                    process::exit(1);
                }
            }
        }
        Some(Commands::CreateDb { path }) => {
            println!("Show not implemented yet. {:?}", path);
            let path = Utf8Path::new(path);
            if !path.exists() {
                println!("Creating database at {:?}", path);
                let parent = path.parent();
                if let Some(parent) = parent {
                    create_dir_all(parent).unwrap();
                    debug!("({}:{}) Created {:?}", function_name!(), line!(), parent);
                }

                let mut dal = Dal::new(path.to_string());
                match init_db(&mut dal.conn) {
                    Ok(_) => {
                        println!("Database created at {:?}", path);
                    }
                    Err(e) => {
                        eprintln!(
                            "Error ({}:{}) Creating database: {:?}",
                            function_name!(),
                            line!(),
                            e
                        );
                        process::exit(1);
                    }
                }
                let _ = dal.clean_table();
            } else {
                eprintln!(
                    "({}:{}) Database already exists at {:?}.",
                    function_name!(),
                    line!(),
                    path
                );
                process::exit(1);
            }
        }
        Some(Commands::Xxx { ids, tags }) => {
            eprintln!("({}:{}) ids: {:?}, tags: {:?}", function_name!(), line!(), ids, tags);
        }
        None => {}
    }
    // Continued program logic goes here...
}

#[cfg(test)]
mod tests {
    use crate::Cli;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert()
    }
}
