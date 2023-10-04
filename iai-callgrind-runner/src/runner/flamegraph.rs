use std::fmt::{Display, Write};
use std::fs::File;
use std::iter::Map;
use std::path::PathBuf;

use inferno::flamegraph::Options;
use log::{trace, warn};

use super::callgrind::parser::{Costs, EventType};
use super::callgrind::CallgrindOutput;
use super::IaiCallgrindError;
use crate::error::Result;

#[derive(Debug, Default, Clone)]
pub struct StackEntry {
    is_inline: bool,
    value: String,
}

impl Display for StackEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_inline {
            f.write_fmt(format_args!("[{}]", self.value))
        } else {
            f.write_str(&self.value)
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Stack {
    pub entries: Vec<StackEntry>,
    pub costs: Costs,
}

impl Stack {
    pub fn new<T>(item: T, costs: Costs, is_inline: bool) -> Self
    where
        T: Into<String>,
    {
        Self {
            entries: vec![StackEntry {
                value: item.into(),
                is_inline,
            }],
            costs,
        }
    }

    pub fn add<T>(&mut self, item: T, costs: Costs, is_inline: bool)
    where
        T: Into<String>,
    {
        self.entries.push(StackEntry {
            value: item.into(),
            is_inline,
        });
        self.costs = costs;
    }

    pub fn contains<T>(&self, item: T, is_inline: bool) -> bool
    where
        T: AsRef<str>,
    {
        let item = item.as_ref();
        self.entries
            .iter()
            .rev()
            .any(|e| e.is_inline == is_inline && e.value == item)
    }

    pub fn to_string(&self, event_type: &EventType) -> Result<String> {
        let mut result = String::new();
        if let Some((first, suffix)) = self.entries.split_first() {
            write!(&mut result, "{first}").unwrap();
            for element in suffix {
                write!(&mut result, ";{element}").unwrap();
            }
            write!(
                &mut result,
                " {}",
                self.costs
                    .cost_by_type(event_type)
                    .ok_or_else(|| IaiCallgrindError::Other(format!(
                        "Error creating flamegraph: Event type '{event_type}' not found"
                    )))?
            )
            .unwrap();
        }

        Ok(result)
    }
}

#[derive(Debug, Default)]
pub struct Stacks(pub Vec<Stack>);

impl Stacks {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push(&mut self, value: Stack) {
        self.0.push(value);
    }

    pub fn add<T>(&mut self, item: T, costs: Costs, is_inline: bool, base: Option<&Stack>)
    where
        T: Into<String>,
    {
        let stack = if let Some(last) = base {
            let mut stack = last.clone();
            stack.add(item, costs, is_inline);
            stack
        } else {
            Stack::new(item, costs, is_inline)
        };

        trace!("Pushing stack: {:?}", stack);
        self.push(stack);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Stack> {
        self.0.iter()
    }

    pub fn last(&self) -> Option<&Stack> {
        self.0.last()
    }
}

pub struct FlamegraphOutput(pub PathBuf);

impl FlamegraphOutput {
    pub fn init(output: &CallgrindOutput) -> Result<Self> {
        let path = output.with_extension("svg").path;
        if path.exists() {
            let old_svg = path.with_extension("svg.old");
            std::fs::copy(&path, &old_svg).map_err(|error| {
                IaiCallgrindError::Other(format!(
                    "Error copying flamegraph file '{}' -> '{}' : {error}",
                    &path.display(),
                    &old_svg.display(),
                ))
            })?;
        }

        Ok(Self(path))
    }

    pub fn create(&self) -> Result<File> {
        File::create(&self.0).map_err(|error| {
            IaiCallgrindError::Other(format!("Creating flamegraph file failed: {error}"))
        })
    }
}

// TODO: MAKE the choice of a title for the svg files configurable??
// TODO: MAKE the choice of a name for the counts configurable??
pub struct Flamegraph {
    pub types: Vec<EventType>,
    pub title: String,
    pub stacks: Stacks,
}

impl Flamegraph {
    pub fn create(&self, dest: &FlamegraphOutput) -> Result<()> {
        if self.stacks.is_empty() {
            warn!("Unable to create a flamegraph: No stacks found");
            return Ok(());
        }

        let output_file = dest.create()?;

        for event_type in &self.types {
            let mut options = Options::default();
            options.title = self.title.clone();
            options.count_name = event_type.to_string();

            let mut stacks = vec![];
            for stack in self.stacks.iter() {
                stacks.push(stack.to_string(event_type)?);
            }

            inferno::flamegraph::from_lines(
                &mut options,
                stacks.iter().map(std::string::String::as_str),
                &output_file,
            )
            .map_err(|error| {
                crate::error::IaiCallgrindError::Other(format!(
                    "Creating flamegraph file failed: {error}"
                ))
            })?;
        }

        Ok(())
    }
}
