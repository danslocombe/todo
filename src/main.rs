extern crate argparse;
extern crate chrono;
extern crate colored;
extern crate rand;
extern crate serde;
extern crate serde_json;
extern crate time;

#[macro_use]
extern crate serde_derive;

use argparse::{ArgumentParser, Store, StoreOption};
use rand::Rng;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::path::PathBuf;
use std::str::FromStr;
use time::Duration;

use std::env;
use std::io::Read;
use std::io::Result as IOResult;
use std::io::Write;

use colored::*;

use chrono::prelude::*;

enum VagueTime {
    Tomorrow,
    Today,
    Evening,
    NextWeek,
    Day(u8),
}

impl FromStr for VagueTime {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use VagueTime::*;
        match s {
            "tomorrow" => Ok(Tomorrow),
            "today" => Ok(Today),
            "tonight" => Ok(Today),
            "evening" => Ok(Evening),
            "week" => Ok(NextWeek),
            "next week" => Ok(NextWeek),
            d => Ok(match u8::from_str(d) {
                Ok(x) => Day(x),
                Err(_e) => {
                    panic!("I don't understand the date you asked for!");
                }
            }),
        }
    }
}

impl VagueTime {
    fn concretise(&self) -> DateTime<Local> {
        use VagueTime::*;
        let t0 = Local::now();
        match self {
            Tomorrow => t0 + Duration::days(1),
            Today => Local::today().and_hms(23, 30, 0),
            Evening => Local::today().and_hms(23, 00, 0),
            NextWeek => t0 + Duration::days(7),
            Day(d) => Local::today()
                .with_day(u32::from(*d))
                .unwrap()
                .and_hms(15, 00, 0),
        }
    }
}

enum Command {
    List,
    Add,
    Started,
    Resolve,
    Remove,
    None,
}

impl FromStr for Command {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Command::*;
        Ok(match s {
            "" => List,
            "list" => List,
            "add" => Add,
            "start" => Started,
            "resolve" => Resolve,
            "remove" => Remove,
            _ => None,
        })
    }
}

fn main() {
    let mut command = Command::List;
    let mut arg = "".to_owned();
    let mut deadline: Option<VagueTime> = None;
    let mut priority: u8 = 0;
    {
        let mut ap = ArgumentParser::new();
        ap.set_description(
            "Something to help me organise\nSupports commands:
list\n - add \"Text of task\"\n - start taskname
 - \nresolve taskname\n - remove taskname

Supports setting deadlines which can be of the form
tommorow, today, tonight, evening, nextweek, or a day of this month as a single
number",
        );
        ap.refer(&mut command)
            .add_argument("command", Store, "Command to run");
        ap.refer(&mut arg)
            .add_argument("arg", Store, "arg for command");
        ap.refer(&mut deadline)
            .add_option(&["-d", "--deadline"], StoreOption, "Deadline of task");
        ap.refer(&mut priority)
            .add_option(&["-p", "--priority"], Store, "Priority of task");
        ap.parse_args_or_exit();
    }

    match command {
        Command::List => {
            do_list();
        }
        Command::Add => {
            do_add(arg, priority, &deadline);
        }
        Command::Started => {
            do_set_progress(&arg, Status::Started);
        }
        Command::Resolve => {
            do_set_progress(&arg, Status::Resolved);
        }
        Command::Remove => {
            do_remove(&arg);
        }
        _ => {
            println!("Unrecognised argument, try todo --help");
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Data {
    entries: Vec<Entry>,
    last_updated: DateTime<Local>,
}

impl Data {
    fn new() -> Self {
        Data {
            entries: Vec::new(),
            last_updated: Local::now(),
        }
    }

    fn add_entry(&mut self, entry: Entry) {
        self.entries.push(entry);
    }

    fn find_entry<'t>(&'t self, id: &str) -> Option<&'t Entry> {
        for x in &self.entries {
            if x.id == id {
                return Some(x);
            }
        }
        None
    }

    fn find_entry_mut<'t>(&'t mut self, id: &str) -> Option<&'t mut Entry> {
        for x in &mut self.entries {
            if x.id == id {
                return Some(x);
            }
        }
        None
    }

    fn remove_by_id(&mut self, id: &str) {
        self.entries.retain(|x| x.id != id);
    }

    fn print(&self) {
        if self.entries.is_empty() {
            println!("Nothing todo, woooooo!");
        }
        for entry in &self.entries {
            println!("{}", entry.format());
        }
    }
}

#[derive(Serialize, Deserialize)]
enum Status {
    NotStarted,
    Started,
    Resolved,
}

impl Status {
    fn is_urgent(&self) -> bool {
        use Status::*;
        match self {
            NotStarted => true,
            Started => true,
            Resolved => false,
        }
    }
    fn to_colored(&self, urgent: &bool) -> ColoredString {
        use Status::*;
        match self {
            NotStarted => {
                let base = "Not Started";
                if *urgent {
                    base.red()
                } else {
                    base.dimmed()
                }
            }
            Started => {
                let base = "Started";
                if *urgent {
                    base.red()
                } else {
                    base.yellow()
                }
            }
            Resolved => "Resolved".green(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Entry {
    id: String,
    task: String,
    deadline: Option<DateTime<Local>>,
    status: Status,
    priority: u8,
}

impl Entry {
    fn new(id: String, task: String, priority: u8, mb_deadline: Option<DateTime<Local>>) -> Self {
        Entry {
            id,
            task,
            deadline: mb_deadline,
            priority,
            status: Status::NotStarted,
        }
    }

    fn format(&self) -> String {

        let deadline_urgent = match self.deadline {
            Some(x) => x.date() <= Local::now().date(),
            _ => false,
        };
        let status_urgent = self.status.is_urgent();
        let urgent = deadline_urgent && status_urgent;

        let deadline_str = match self.deadline {
            Some(deadline) => {
                let str = format!("{}", deadline.format("\n\t Deadline: %d-%m %H:%M")).to_owned();
                if urgent {
                    str.red()
                } else {
                    str.dimmed()
                }
            }
            None => "".to_owned().dimmed(),
        };

        let priority_str = if self.priority > 0 {
            format!("Priority: {}", self.priority).to_owned()
        } else {
            "".to_owned()
        };

        let status_str = self.status.to_colored(&urgent);

        format!(
            "Task: {} {} | {} | {} {}",
            self.id,
            priority_str,
            self.task.bold(),
            status_str,
            deadline_str
        )
    }
}

const DATA_FOLDER:    &str = ".todo.d";
const DATA_FILENAME:  &str = "data.json";
const NOUNS_FILENAME: &str = "nouns.txt";

fn data_folder() -> PathBuf {
    match env::home_dir() {
        Some(mut p) => {
            p.push(DATA_FOLDER);
            p
        }
        None => {
            panic!("Couldn't find your home folder, setup will require some manual hacking");
        }
    }
}

fn data_path() -> PathBuf {
    let mut p = data_folder();
    p.push(DATA_FILENAME);
    p
}

fn nouns_path() -> PathBuf {
    let mut p = data_folder();
    p.push(NOUNS_FILENAME);
    p
}

fn load_data() -> IOResult<Data> {
    let filename = data_path();
    let mut file = File::open(filename)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    Ok(match serde_json::from_str(&contents) {
        Ok(d) => d,
        Err(e) => {
            panic!("Error, corrupted data!\n{}", e);
        }
    })
}

fn load_data_catch() -> Data {
    load_data().unwrap_or_else(|_| Data::new())
}

fn load_nouns() -> IOResult<Vec<String>> {
    let filename = nouns_path();
    let f = File::open(filename)?;
    let f = BufReader::new(f);

    f.lines().collect()
}

fn save_data(data: &Data) -> Result<(), serde_json::Error> {
    let j = serde_json::to_string(data)?;
    let filename = data_path();

    // TODO merge two result error types
    let mut file = OpenOptions::new()
                .write(true) // Overwrite whole file when writing
                .create(true)
                .truncate(true) // Remove any previous stuff
                .open(filename).unwrap();
    file.write_all(j.as_ref()).unwrap();

    Ok(())
}

fn do_list() {
    let data = load_data_catch();
    data.print();
}

fn pick_name(data: &Data) -> String {
    // TODO error handle
    let nouns = load_nouns().unwrap();
    let mut noun;

    // We know this will probably terminate
    // stop worrying guys
    #[allow(while_immutable_condition)]
    while {
        noun = rand::thread_rng().choose(&nouns).unwrap();

        // Repeat until we find one not already used
        data.find_entry(noun).is_some()
    } {}

    noun.to_owned()
}

fn do_add(task: String, priority: u8, deadline_vague: &Option<VagueTime>) {
    let mut data = load_data_catch();
    let id = pick_name(&data);
    println!("Adding {} - '{}'", id, task);

    let deadline = deadline_vague.as_ref().map(|x| x.concretise());
    let new_entry = Entry::new(id, task, priority, deadline);

    data.add_entry(new_entry);
    data.print();

    save_data(&data).unwrap();
}

fn do_set_progress(id: &str, progress: Status) {
    let mut data = load_data_catch();
    println!("Resolving '{}'", id);
    {
        // Scope for mutable borrow
        match data.find_entry_mut(id) {
            Some(entry) => {
                entry.status = progress;
            }
            None => {
                println!("Could not find '{}' to update, exiting..", id);
                return;
            }
        }
    }
    data.print();
    save_data(&data).unwrap();
}

fn do_remove(id: &str) {
    let mut data = load_data_catch();
    println!("Removing '{}'", id);
    data.remove_by_id(id);

    data.print();
    save_data(&data).unwrap();
}
