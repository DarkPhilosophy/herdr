use std::io::Read;

use crate::api::schema::{Method, Request, SidebarReportSectionParams};

pub(super) fn run_sidebar_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(|arg| arg.as_str()) else {
        print_sidebar_help();
        return Ok(2);
    };

    match subcommand {
        "report-section" => sidebar_report_section(&args[1..]),
        "help" | "--help" | "-h" => {
            print_sidebar_help();
            Ok(0)
        }
        _ => {
            print_sidebar_help();
            Ok(2)
        }
    }
}

fn sidebar_report_section(args: &[String]) -> std::io::Result<i32> {
    if args.len() == 1 && matches!(args[0].as_str(), "help" | "--help" | "-h") {
        print_report_section_usage();
        return Ok(0);
    }
    if args.len() != 1 || args[0] != "--stdin" {
        print_report_section_usage();
        return Ok(2);
    }

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let params: SidebarReportSectionParams = match serde_json::from_str(&input) {
        Ok(params) => params,
        Err(err) => {
            eprintln!("invalid sidebar report-section params JSON: {err}");
            return Ok(2);
        }
    };

    super::print_response(&super::send_request(&Request {
        id: "cli:sidebar:report-section".into(),
        method: Method::SidebarReportSection(params),
    })?)
}

fn print_report_section_usage() {
    eprintln!("usage: herdr sidebar report-section --stdin");
}

fn print_sidebar_help() {
    eprintln!("herdr sidebar commands:");
    eprintln!("  herdr sidebar report-section --stdin");
}
