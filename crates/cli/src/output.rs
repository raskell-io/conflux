//! Output formatting for CLI commands.

use serde::Serialize;

/// Output format for CLI results.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// Plain text output (default).
    #[default]
    Plain,
    /// JSON output.
    Json,
    /// YAML output.
    Yaml,
}

/// Prints a value in the specified format.
pub fn print_value<T: Serialize + std::fmt::Display>(value: &T, format: OutputFormat) {
    match format {
        OutputFormat::Plain => println!("{value}"),
        OutputFormat::Json => {
            if let Ok(json) = serde_json::to_string_pretty(value) {
                println!("{json}");
            }
        }
        OutputFormat::Yaml => {
            if let Ok(yaml) = serde_yaml::to_string(value) {
                print!("{yaml}");
            }
        }
    }
}

/// Prints a serializable value in the specified format.
pub fn print_json<T: Serialize>(value: &T, format: OutputFormat) {
    match format {
        OutputFormat::Plain | OutputFormat::Json => {
            if let Ok(json) = serde_json::to_string_pretty(value) {
                println!("{json}");
            }
        }
        OutputFormat::Yaml => {
            if let Ok(yaml) = serde_yaml::to_string(value) {
                print!("{yaml}");
            }
        }
    }
}

/// Prints a success message.
pub fn print_success(message: &str) {
    println!("{message}");
}

/// Prints an error message to stderr.
pub fn print_error(message: &str) {
    eprintln!("error: {message}");
}

/// Prints a warning message to stderr.
pub fn print_warning(message: &str) {
    eprintln!("warning: {message}");
}

/// A table builder for simple tabular output.
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    /// Creates a new table with the given headers.
    pub fn new(headers: Vec<&str>) -> Self {
        Self {
            headers: headers.into_iter().map(String::from).collect(),
            rows: Vec::new(),
        }
    }

    /// Adds a row to the table.
    pub fn add_row(&mut self, row: Vec<String>) {
        self.rows.push(row);
    }

    /// Prints the table to stdout.
    pub fn print(&self) {
        if self.headers.is_empty() {
            return;
        }

        // Calculate column widths
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.len()).collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        // Print headers
        let header_line: String = self
            .headers
            .iter()
            .zip(&widths)
            .map(|(h, w)| format!("{:width$}", h, width = w))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{header_line}");

        // Print separator
        let separator: String = widths.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>().join("  ");
        println!("{separator}");

        // Print rows
        for row in &self.rows {
            let line: String = row
                .iter()
                .zip(&widths)
                .map(|(c, w)| format!("{:width$}", c, width = w))
                .collect::<Vec<_>>()
                .join("  ");
            println!("{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_formatting() {
        let mut table = Table::new(vec!["ID", "Name", "Status"]);
        table.add_row(vec!["1".into(), "foo".into(), "active".into()]);
        table.add_row(vec!["2".into(), "bar".into(), "inactive".into()]);
        // Just verify it doesn't panic
        table.print();
    }
}
