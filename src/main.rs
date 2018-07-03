extern crate chrono;
#[macro_use]
extern crate clap;
extern crate rprompt;

use chrono::prelude::*;
use clap::{Arg, SubCommand};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let matches = app_from_crate!()
        .subcommand(
            SubCommand::with_name("ask")
                .about("Ask for status of all habits for a day")
                .arg(Arg::with_name("days ago").index(1).default_value("1")),
        )
        .subcommand(SubCommand::with_name("log").about("Print habit log"))
        .subcommand(SubCommand::with_name("todo").about("Print unresolved tasks for today"))
        .get_matches();

    let mut tick = Tick::new();
    tick.load();

    match matches.subcommand() {
        ("log", Some(_)) => tick.log(),
        ("todo", Some(_)) => tick.todo(),
        ("ask", Some(sub_matches)) => {
            let ago: i64 = sub_matches.value_of("days ago").unwrap().parse().unwrap();
            tick.ask(ago);
            tick.log();
        }
        _ => {
            // no subcommand used
            tick.ask(1);
            tick.log();
        }
    }
}

struct Tick {
    habits_file: PathBuf,
    log_file: PathBuf,
    habits: Vec<Habit>,
    log: HashMap<String, Vec<(Date<Local>, String)>>,
    entries: Vec<Entry>,
}

#[derive(PartialEq)]
enum DayStatus {
    Unknown,
    NotDone,
    Done,
    Satisfied,
}

impl Tick {
    fn new() -> Tick {
        let mut tick_dir = env::home_dir().unwrap();
        tick_dir.push(".tick");
        if !tick_dir.is_dir() {
            println!("Creating {:?}", tick_dir);
            fs::create_dir(&tick_dir).unwrap();
        }

        let mut habits_file = tick_dir.clone();
        habits_file.push("habits");
        if !habits_file.is_file() {
            fs::File::create(&habits_file).unwrap();
        }

        let mut log_file = tick_dir.clone();
        log_file.push("log");
        if !log_file.is_file() {
            fs::File::create(&log_file).unwrap();
        }

        Tick {
            habits_file,
            log_file,
            habits: vec![],
            log: HashMap::new(),
            entries: vec![],
        }
    }

    fn load(&mut self) {
        self.log = self.get_log();
        self.habits = self.get_habits();
        self.entries = self.get_entries();
    }

    fn entry(&self, entry: &Entry) {
        let file = OpenOptions::new()
            .append(true)
            .open(&self.log_file)
            .unwrap();

        let last_date = self.last_date();

        if let Some(last_date) = last_date {
            if last_date != entry.date {
                write!(&file, "\n").unwrap();
            }
        }

        write!(
            &file,
            "{}\t{}\t{}\n",
            &entry.date.format("%F"),
            &entry.habit,
            &entry.value
        ).unwrap();
    }

    fn log(&self) {
        let to = Local::now();
        let from = to.checked_sub_signed(chrono::Duration::days(60)).unwrap();

        print!("{0: >25} ", "");
        let mut current = from;
        while current <= to {
            print!("{0: >1}", self.spark(self.get_score(&current.date())));
            current = current
                .checked_add_signed(chrono::Duration::days(1))
                .unwrap();
        }
        println!();

        for habit in self.habits.iter() {
            print!("{0: >25} ", habit.name);

            let mut current = from;
            while current <= to {
                print!(
                    "{0: >1}",
                    &self.status_to_symbol(&self.day_status(&habit, &current.date()))
                );

                current = current
                    .checked_add_signed(chrono::Duration::days(1))
                    .unwrap();
            }
            println!();
        }

        let date = to
            .checked_sub_signed(chrono::Duration::days(1))
            .unwrap()
            .date();
        println!("Yesterday's score: {}%", self.get_score(&date));
    }

    fn get_habits(&self) -> Vec<Habit> {
        let f = File::open(&self.habits_file).unwrap();
        let file = BufReader::new(&f);

        let mut habits = vec![];

        for line in file.lines() {
            let l = line.unwrap();

            if l.chars().next().unwrap() != '#' {
                let split = l.splitn(2, " ");
                let parts: Vec<&str> = split.collect();

                habits.push(Habit {
                    every_days: parts[0].parse().unwrap(),
                    name: String::from(parts[1]),
                });
            }
        }

        habits
    }

    fn ask(&self, ago: i64) {
        let from = Local::now()
            .checked_sub_signed(chrono::Duration::days(ago))
            .unwrap()
            .date();

        let now = Local::now().date();

        let mut current = from;
        while current <= now {
            println!("{}:", &current);

            for habit in self.get_todo(&current) {
                let l = format!("{}? [y/n/-] ", &habit.name);

                let mut value;
                loop {
                    value = String::from(rprompt::prompt_reply_stdout(&l).unwrap());
                    value = value.trim_right().to_string();

                    if value == "y" || value == "n" || value == "" {
                        break;
                    }
                }

                if value != "" {
                    self.entry(&Entry {
                        date: current.clone(),
                        habit: habit.name,
                        value,
                    });
                }
            }

            current = current
                .checked_add_signed(chrono::Duration::days(1))
                .unwrap();
        }
    }

    fn todo(&self) {
        let entry_date = Local::now().date();

        for habit in self.get_todo(&entry_date) {
            println!("{}", &habit.name);
        }
    }

    fn get_todo(&self, todo_date: &Date<Local>) -> Vec<Habit> {
        let mut habits = self.get_habits();

        habits.retain(|h| {
            if self.log.contains_key(&h.name.clone()) {
                let mut iter = self.log.get(&h.name.clone()).unwrap().iter();

                if let Some(_) = iter.find(|(date, _value)| date == todo_date) {
                    return false;
                }
            }
            return true;
        });

        habits
    }

    fn get_entries(&self) -> Vec<Entry> {
        let f = File::open(&self.log_file).unwrap();
        let file = BufReader::new(&f);

        let mut entries = vec![];

        for line in file.lines() {
            let l = line.unwrap();
            if l == "" {
                continue;
            }
            let split = l.split("\t");
            let parts: Vec<&str> = split.collect();

            let date_str = format!("{}T00:00:00+00:00", parts[0]);

            let entry = Entry {
                date: DateTime::parse_from_rfc3339(&date_str)
                    .unwrap()
                    .with_timezone(&Local)
                    .date(),
                habit: parts[1].to_string(),
                value: parts[2].to_string(),
            };

            entries.push(entry);
        }

        entries
    }

    fn get_entry(&self, date: &Date<Local>, habit: &String) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|entry| entry.date == *date && entry.habit == *habit)
    }

    fn day_status(&self, habit: &Habit, date: &Date<Local>) -> DayStatus {
        if let Some(entry) = self.get_entry(&date, &habit.name) {
            if entry.value == "y" {
                DayStatus::Done
            } else {
                if self.habit_satisfied(habit, &date) {
                    DayStatus::Satisfied
                } else {
                    DayStatus::NotDone
                }
            }
        } else {
            DayStatus::Unknown
        }
    }

    fn status_to_symbol(&self, status: &DayStatus) -> String {
        let symbol = match status {
            DayStatus::Unknown => "•",
            DayStatus::NotDone => " ",
            DayStatus::Done => "━",
            DayStatus::Satisfied => "─",
        };
        String::from(symbol)
    }

    fn habit_satisfied(&self, habit: &Habit, date: &Date<Local>) -> bool {
        if habit.every_days < 1 {
            return false;
        }

        let from = date
            .checked_sub_signed(chrono::Duration::days(habit.every_days - 1))
            .unwrap();
        let mut current = from;
        while current <= *date {
            if let Some(entry) = self.get_entry(&current, &habit.name) {
                if entry.value == "y" {
                    return true;
                }
            }
            current = current
                .checked_add_signed(chrono::Duration::days(1))
                .unwrap();
        }
        false
    }

    fn get_log(&self) -> HashMap<String, Vec<(Date<Local>, String)>> {
        let mut log = HashMap::new();

        for entry in self.get_entries() {
            if !log.contains_key(&entry.habit) {
                log.insert(entry.habit.clone(), vec![]);
            }
            log.get_mut(&entry.habit)
                .unwrap()
                .push((entry.date, entry.value));
        }

        log
    }

    fn last_date(&self) -> Option<Date<Local>> {
        self.get_entries()
            .last()
            .and_then(|entry| Some(entry.date.clone()))
    }

    fn get_score(&self, score_date: &Date<Local>) -> f32 {
        let mut todo: Vec<bool> = self
            .habits
            .iter()
            .map(|habit| habit.every_days > 0)
            .collect();
        todo.retain(|value| *value);

        let mut done: Vec<bool> = self
            .habits
            .iter()
            .map(|habit| {
                let status = self.day_status(&habit, &score_date);
                habit.every_days > 0
                    && (status == DayStatus::Done || status == DayStatus::Satisfied)
            })
            .collect();
        done.retain(|value| *value);

        100.0 * done.len() as f32 / todo.len() as f32
    }

    fn spark(&self, score: f32) -> String {
        let sparks = vec![" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
        let i = (score / sparks.len() as f32) as usize;
        String::from(sparks[i])
    }
}

struct Entry {
    date: Date<Local>,
    habit: String,
    value: String,
}

struct Habit {
    every_days: i64,
    name: String,
}
