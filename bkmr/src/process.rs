use std::{fs, io};

use anyhow::{Context};
use std::fs::File;
use std::io::Write;
use std::process::{Command, Stdio};

use indoc::formatdoc;
use log::{debug, error};
use regex::Regex;
use stdext::function_name;

use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

use crate::dal::Dal;
use crate::environment::CONFIG;
use crate::helper;
use crate::helper::abspath;
use crate::models::Bookmark;

pub fn show_bms(bms: &Vec<Bookmark>) {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    let first_col_width = bms.len().to_string().len();

    for (i, bm) in bms.iter().enumerate() {
        stdout
            .set_color(ColorSpec::new().set_fg(Some(Color::Green)))
            .unwrap();
        write!(&mut stdout, "{:first_col_width$}. {}", i + 1, bm.metadata).unwrap();
        stdout
            .set_color(ColorSpec::new().set_fg(Some(Color::White)))
            .unwrap();
        write!(&mut stdout, " [{}]\n", bm.id).unwrap();

        stdout
            .set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))
            .unwrap();
        writeln!(&mut stdout, "{:first_col_width$}  {}", "", bm.URL).unwrap();

        if bm.desc != "" {
            stdout
                .set_color(ColorSpec::new().set_fg(Some(Color::White)))
                .unwrap();
            writeln!(&mut stdout, "{:first_col_width$}  {}", "", bm.desc).unwrap();
        }

        let tags = bm.tags.replace(",", " ");
        if tags.find(|c: char| !c.is_whitespace()).is_some() {
            stdout
                .set_color(ColorSpec::new().set_fg(Some(Color::Blue)))
                .unwrap();
            writeln!(&mut stdout, "{:first_col_width$}  {}", "", tags.trim()).unwrap();
        }

        stdout.reset().unwrap();
        println!();
    }
}

fn parse(input: &str) -> Vec<String> {
    let binding = input.trim().replace(",", "").to_lowercase();
    let tokens = binding
        .split(" ")
        .filter(|s| *s != "")
        .map(|s| s.to_string())
        .collect();
    debug!("({}:{}) {:?}", function_name!(), line!(), tokens);
    tokens
}

pub fn process(bms: &Vec<Bookmark>) {
    // debug!("({}:{}) {:?}", function_name!(), line!(), bms);
    let help_text = r#"
        <n1> <n2>:      opens selection in browser
        p <n1> <n2>:    print id-list of selection
        p:              print all ids
        d <n1> <n2>:    delete selection
        e:              edit selection
        q | ENTER:      quit
        h:              help
    "#;

    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let mut tokens = parse(&input);
        if tokens.len() == 0 {
            break;
        }

        let regex = Regex::new(r"^\d+").unwrap(); // Create a new Regex object
        match tokens[0].as_str() {
            "p" => {
                let ids = helper::ensure_int_vector(&tokens.split_off(1));
                if ids.is_none() {
                    error!("({}:{}) Invalid input, only numbers allowed {:?}", function_name!(), line!(), ids);
                    continue;
                }

                print_ids(ids.unwrap(), bms.clone()).unwrap_or_else(|e| {
                    error!("({}:{}) {}", function_name!(), line!(), e);
                });
                break;
            }
            "d" => {
                let ids = helper::ensure_int_vector(&tokens.split_off(1));
                if ids.is_none() {
                    error!("({}:{}) Invalid input, only numbers allowed {:?}", function_name!(), line!(), ids);
                    continue;
                }

                delete_bms(ids.unwrap(), bms.clone()).unwrap_or_else(|e| {
                    error!("({}:{}) {}", function_name!(), line!(), e);
                });
                break;
            }
            "e" => {
                let ids = helper::ensure_int_vector(&tokens.split_off(1));
                if ids.is_none() {
                    error!("({}:{}) Invalid input, only numbers allowed {:?}", function_name!(), line!(), ids);
                    continue;
                }
                edit_bms(ids.unwrap(), bms.clone()).unwrap_or_else(|e| {
                    error!("({}:{}) {}", function_name!(), line!(), e);
                });
                break;
            }
            "h" => println!("{}", help_text),
            "q" => break,
            // Use Regex object in a guard
            s if regex.is_match(s) => {
                let ids = helper::ensure_int_vector(&tokens);
                if ids.is_none() {
                    error!("({}:{}) Invalid input, only numbers allowed {:?}", function_name!(), line!(), ids);
                    continue;
                }
                open_bms(ids.unwrap(), bms.clone()).unwrap_or_else(|e| {
                    error!("({}:{}) {}", function_name!(), line!(), e);
                });
                break;
            }
            _ => {
                println!("{}", "Invalid Input");
                println!("{}", help_text);
            }
        }
    }
}

pub fn edit_bms(ids: Vec<i32>, bms: Vec<Bookmark>) -> anyhow::Result<()> {
    debug!("({}:{}) {:?}", function_name!(), line!(), ids);
    do_sth_with_bms(ids, bms, do_edit)
        .with_context(|| format!("({}:{}) Error opening bookmarks", function_name!(), line!()))?;
    Ok(())
}

fn open_bm(bm: &Bookmark) -> anyhow::Result<()> {
    _open_bm(&bm.URL)?;
    Ok(())
}

fn _open_bm(uri: &str) -> anyhow::Result<()> {
    if uri.starts_with("shell::") {
        let cmd = uri.replace("shell::", "");
        debug!("({}:{}) Shell Command {:?}", function_name!(), line!(), cmd);
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("({}:{}) Error opening {}", function_name!(), line!(), uri))?;

        let status = child.wait().expect("Failed to wait on Vim");
        debug!("({}:{}) Exit status from command: {:?}", function_name!(), line!(), status);
        Ok(())
    } else {
        debug!("({}:{}) General OS open {:?}", function_name!(), line!(), uri);
        // todo error propagation upstream not working
        match abspath(uri) {
            Some(p) => {
                open::that(p)?;
            }
            None => {
                open::that(uri)?;
            }
        }
        Ok(())
    }
}

pub fn open_bms(ids: Vec<i32>, bms: Vec<Bookmark>) -> anyhow::Result<()> {
    debug!("({}:{}) {:?}", function_name!(), line!(), ids);

    do_sth_with_bms(ids, bms, open_bm)
        .with_context(|| format!("({}:{}) Error opening bookmarks", function_name!(), line!()))?;
    Ok(())
}

pub fn delete_bms(mut ids: Vec<i32>, bms: Vec<Bookmark>) -> anyhow::Result<()> {
    // reverse sort necessary due to DB compaction (deletion of last entry first)
    ids.reverse();
    debug!("({}:{}) {:?}", function_name!(), line!(), &ids);
    fn delete_bm(bm: &Bookmark) -> anyhow::Result<()> {
        let _ = Dal::new(CONFIG.db_url.clone()).delete_bookmark2(bm.id)?;
        Ok(())
    }
    do_sth_with_bms(ids, bms, delete_bm).with_context(|| {
        format!(
            "({}:{}) Error deleting bookmarks",
            function_name!(),
            line!()
        )
    })?;
    Ok(())
}

fn do_sth_with_bms(
    ids: Vec<i32>,
    bms: Vec<Bookmark>,
    do_sth: fn(bm: &Bookmark) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    debug!("({}:{}) {:?}", function_name!(), line!(), ids);
    for id in ids {
        if id as usize <= bms.len() {
            let bm = &bms[id as usize - 1];
            debug!(
                    "({}:{}) {:?}: bm {:?}",
                    function_name!(),
                    line!(),
                    id,
                    bm
                );
            do_sth(bm).with_context(|| {
                format!(
                    "({}:{}): bm {:?}",
                    function_name!(),
                    line!(),
                    bm
                )
            })?;
        }
    }
    Ok(())
}

pub fn do_edit(bm: &Bookmark) -> anyhow::Result<()> {
    // Create a file inside of `std::env::temp_dir()`.
    // let mut file = tempfile()?;
    let mut temp_file = File::create("temp.txt")?;

    let template = formatdoc! {r###"
        # Lines beginning with "#" will be stripped.
        # Add URL in next line (single line).
        {url}
        # Add TITLE in next line (single line). Leave blank to web fetch, "-" for no title.
        {title}
        # Add comma-separated TAGS in next line (single line).
        {tags}
        # Add COMMENTS in next line(s). Leave blank to web fetch, "-" for no comments.
        {comments}
        "###,
        url=bm.URL.clone(),
        title=bm.metadata.clone(),
        tags=bm.tags.clone(),
        comments=bm.desc.clone(),
    };

    temp_file.write_all(template.as_bytes()).with_context(|| {
        format!(
            "({}:{}) Error writing to temp file",
            function_name!(),
            line!()
        )
    })?;

    // Open the temporary file with Vim
    Command::new("vim")
        .arg("temp.txt")
        .status()
        .with_context(|| {
            format!(
                "({}:{}) Error opening temp file with vim",
                function_name!(),
                line!()
            )
        })?;

    // Read the modified content of the file back into a string
    let modified_content = fs::read_to_string("temp.txt")
        .with_context(|| format!("({}:{}) Error reading temp file", function_name!(), line!()))?;
    let lines: Vec<&str> = modified_content
        .split("\n")
        .filter(|l| !l.starts_with("#"))
        .collect();
    let new_bm = Bookmark {
        id: bm.id,
        URL: lines[0].to_string(),
        metadata: lines[1].to_string(), // title
        tags: lines[2].to_string(),
        desc: lines[3].to_string(), // comments
        flags: bm.flags,
        last_update_ts: Default::default(), // will be overwritten by diesel
    };
    println!("Modified content: {}", modified_content);
    println!("lines: {:?}", lines);

    // let mut dal = Dal::new(String::from("../db/bkmr.db"));
    Dal::new(CONFIG.db_url.clone())
        .update_bookmark(new_bm)
        .with_context(|| format!("({}:{}) Error updating bookmark", function_name!(), line!()))?;
    // Delete the temporary file
    fs::remove_file("temp.txt")?;
    Ok(())
}

fn print_ids(ids: Vec<i32>, bms: Vec<Bookmark>) -> anyhow::Result<()> {
    debug!("({}:{}) ids: {:?}", function_name!(), line!(), ids);
    let ids = if ids.len() == 0 {
        (1..=bms.len() as i32).collect()
    } else {
        ids
    };
    let ids_str: Vec<String> = ids.iter().map(|x| x.to_string()).collect();
    println!("{}", ids_str.join(" "));
    Ok(())
}

#[cfg(test)]
mod test {
    use anyhow::anyhow;
    use rstest::*;

    use crate::dal::Dal;
    use crate::helper::init_db;

    use super::*;

    #[ctor::ctor]
    fn init() {
        let _ = env_logger::builder()
            // Include all events in tests
            .filter_level(log::LevelFilter::max())
            // Ensure events are captured by `cargo test`
            .is_test(true)
            // Ignore errors initializing the logger if tests race to configure it
            .try_init();
    }

    #[fixture]
    fn bms() -> Vec<Bookmark> {
        let mut dal = Dal::new(String::from("../db/bkmr.db"));
        init_db(&mut dal.conn).expect("Error DB init");
        let bms = dal.get_bookmarks("");
        bms.unwrap()
    }

    #[rstest]
    #[ignore = "Manual Test"]
    fn test_process(bms: Vec<Bookmark>) {
        process(&mut bms.clone());
    }

    #[rstest]
    fn test_show_bms(bms: Vec<Bookmark>) {
        show_bms(&bms);
    }

    // Config is for Makefile tests. DO NOT RUN HERE
    #[rstest]
    #[ignore = "Manual Test with Makefile"]
    #[case("https://www.google.com")]
    #[ignore = "Manual Test with Makefile"]
    #[case("./tests/resources/bkmr.pptx")]
    #[ignore = "Manual Test with Makefile"]
    #[case(r#####"shell::vim +/"## SqlAlchemy" $HOME/dev/s/private/bkmr/bkmr/tests/resources/sample_docu.md"#####)]
    fn test_open_bm(#[case] bm: &str) {
        _open_bm(bm).unwrap();
    }

    #[rstest]
    #[ignore = "Manual Test"]
    #[case(vec ! [1])]
    fn test_open_bms(bms: Vec<Bookmark>, #[case] ids: Vec<i32>) {
        open_bms(ids, bms).unwrap();
    }

    #[rstest]
    // #[case(vec ! [String::from("1")])]
    #[case(vec ! [])]
    fn test_print_ids(bms: Vec<Bookmark>, #[case] tokens: Vec<i32>) {
        print_ids(tokens, bms).unwrap();
    }  // todo assert missing

    #[rstest]
    #[case(vec ! [1])]
    fn test_do_sth_with_bms(#[case] tokens: Vec<i32>, bms: Vec<Bookmark>) {
        let result = do_sth_with_bms(tokens, bms, |bm| {
            println!("FN: {:?}", bm);
            Ok(())
        });
        assert!(result.is_ok());
    }  // todo assert missing

    #[rstest]
    fn test_do_sth_with_bms_error(bms: Vec<Bookmark>) {
        let result = do_sth_with_bms(vec![1 as i32], bms, |bm| {
            println!("FN: {:?}", bm);
            Err(anyhow!("Anyhow Error"))
        });
        assert!(result.is_err());
    }
}
