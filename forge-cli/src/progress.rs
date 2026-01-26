use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Progress tracking for survey operations
pub struct SurveyProgress {
    multi: MultiProgress,
    main_bar: ProgressBar,
    current_repo_bar: Option<ProgressBar>,
}

impl SurveyProgress {
    pub fn new(total_repos: u64) -> Self {
        let multi = MultiProgress::new();

        let main_bar = multi.add(ProgressBar::new(total_repos));
        main_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} repos ({msg})")
                .unwrap()
                .progress_chars("█▓▒░"),
        );

        Self {
            multi,
            main_bar,
            current_repo_bar: None,
        }
    }

    pub fn start_repo(&mut self, repo_name: &str) {
        self.main_bar.set_message(repo_name.to_string());

        // Create sub-progress for current repo
        let bar = self.multi.add(ProgressBar::new_spinner());
        bar.set_style(
            ProgressStyle::default_spinner()
                .template("  {spinner:.green} {msg}")
                .unwrap(),
        );
        bar.set_message(format!("Processing {}...", repo_name));

        self.current_repo_bar = Some(bar);
    }

    pub fn finish_repo(&mut self) {
        if let Some(bar) = self.current_repo_bar.take() {
            bar.finish_and_clear();
        }
        self.main_bar.inc(1);
    }

    pub fn set_repo_status(&self, status: &str) {
        if let Some(ref bar) = self.current_repo_bar {
            bar.set_message(status.to_string());
        }
    }

    pub fn finish(&self) {
        self.main_bar.finish_with_message("Survey complete!");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_creation() {
        let progress = SurveyProgress::new(5);
        // Basic smoke test - just ensure we can create progress bars
        progress.finish();
    }

    #[test]
    fn test_progress_repo_tracking() {
        let mut progress = SurveyProgress::new(3);
        progress.start_repo("repo1");
        progress.set_repo_status("Parsing...");
        progress.finish_repo();

        progress.start_repo("repo2");
        progress.finish_repo();

        progress.finish();
    }
}
