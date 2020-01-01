use std::time::{Duration, Instant};

/// Stores some helpful metrics to uncover what is happening during execution.
#[derive(Debug)]
pub(crate) struct TimeLog {
    start_instant: Instant,

    /// Duration of the search logic, which encapsulates
    /// walking the filesystem, regex matching, and sending
    /// results to the printer (but not the printing itself).
    pub(crate) search_duration: Option<Duration>,

    /// Duration of printing, measured from the very first message received
    /// by the printer, to the very last completes printing.
    pub(crate) print_duration: Option<Duration>,

    /// The duration between when the printer was spawned,
    /// and when the first result arrived for printing.
    pub(crate) printer_spawn_to_print: Option<Duration>,

    /// The duratio between the first result arriving at the printer,
    /// and the first actual printing. This will be significant when grouping
    /// by file is enabled (must complete a whole file before we can print it).
    pub(crate) first_result_to_first_print: Option<Duration>,

    /// Duration from start of execution until end.
    /// (Functionally, top of `main` until end of `main`, after joining the
    /// printing thread, but which may be fuzzy due to async).
    pub(crate) start_die_duration: Option<Duration>,
}

impl TimeLog {
    pub(crate) fn new(start_instant: Instant) -> Self {
        TimeLog {
            start_instant,
            search_duration: None,
            print_duration: None,
            printer_spawn_to_print: None,
            first_result_to_first_print: None,
            start_die_duration: None,
        }
    }

    pub(crate) fn log_search_duration(&mut self) {
        assert!(self.search_duration.is_none());

        self.search_duration = Some(self.start_instant.elapsed());
    }

    pub(crate) fn log_print_duration(&mut self) {
        assert!(self.print_duration.is_none());

        self.print_duration = Some(self.start_instant.elapsed());
    }

    pub(crate) fn log_printer_spawn_to_print(&mut self) {
        assert!(self.printer_spawn_to_print.is_none());

        self.printer_spawn_to_print = Some(self.start_instant.elapsed());
    }

    pub(crate) fn log_start_die_duration(&mut self) {
        assert!(self.start_die_duration.is_none());

        self.start_die_duration = Some(self.start_instant.elapsed());
    }
}
