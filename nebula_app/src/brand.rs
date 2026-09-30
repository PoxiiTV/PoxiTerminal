pub const NAME: &str = "PoxiTerminal";
pub const WINDOWS_APP_ID: &str = "com.poxiitv.poxiterminal";
pub const DESCRIPTION: &str = "PoxiTerminal — terminal acelerado por GPU";

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn display_name_and_cli_use_poxiterminal() {
        let mut command = crate::cli::Options::command();
        assert_eq!(command.get_name(), super::NAME);
        assert_eq!(command.get_bin_name(), Some("poxiterminal"));
        assert!(command.render_version().starts_with(super::NAME));
        assert!(command.render_long_help().to_string().contains(super::DESCRIPTION));
        assert_eq!(crate::config::window::Identity::default().title, super::NAME);
        assert_eq!(crate::config::window::DEFAULT_CLASS, "PoxiTerminal");
    }
}
