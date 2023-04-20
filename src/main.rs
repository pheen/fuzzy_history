// todo:
// - score by times_selected and exit_code

use console::{measure_text_width, Key, Term};
use dialoguer::theme::Theme;
use dialoguer::{theme::ColorfulTheme, theme::SimpleTheme, Select};
use fuzzy_matcher::FuzzyMatcher;
use std::fs;
use std::path::Path;
use std::{fmt, io, ops::Rem};

#[macro_use]
extern crate tantivy;
use home::home_dir;
use tantivy::collector::{Count, TopDocs};
use tantivy::directory::MmapDirectory;
use tantivy::query::{BooleanQuery, Occur, QueryParser, RegexQuery, TermQuery};
use tantivy::Directory;
use tantivy::Index;
use tantivy::ReloadPolicy;
use tantivy::{schema::*, DocId, Score, SegmentReader};

use regex::Regex;
use std::env;

use ulid::Ulid;

use indoc::concatdoc;

#[macro_use]
extern crate lazy_static;

const HELP: &str = concatdoc! {"
    Usage: ", env!("CARGO_BIN_NAME"), " [options]

    Options:
        import         Index command history for the current shell
        delete_index   Remove all indexed command history

    Developer Options:
        search         Start a search client, the same as what's invoked from the keybind ^R
        add            Write a command to the index (expects the format \"<exit code>:<command>\")

    Notes:
        - Only Zsh is supported
        - All persistent data is stored in ~/.fuzzy_matcher

    For setup and full documentation, see: https://github.com/pheen/fuzzy_history
"};

fn indexable_command(text: &str) -> bool {
    lazy_static! {
        static ref RE: Regex = Regex::new("\\d+:.*+").unwrap();
    }
    RE.is_match(text)
}

use backtrace_on_stack_overflow;

fn main() -> std::io::Result<()> {
    unsafe { backtrace_on_stack_overflow::enable() };

    let cmd = env::args().nth(1).unwrap_or("".to_string());

    match cmd.as_str() {
        "add" => {
            let new_command = env::args().nth(2).unwrap_or("".to_string());

            if indexable_command(new_command.as_str()) {
                index_command(new_command).unwrap();
            } else {
                println!("Indexing failed, the command does't match the pattern \"<exit code>:<command>\"");
                println!("Failed input:{:#?}", new_command);
                std::process::exit(1);
            }
        }
        "search" => {
            let fd_path = env::args().nth(2).unwrap_or("".to_string());
            let initial_input = env::args().nth(3).unwrap_or("".to_string());

            if let Ok(selection) = interactive_search_command(fd_path, initial_input) {
                // This is captured by `fzh-widget` in fzh.zsh then executed as a
                // shell command.
                println!("{}", selection);
            }
        }
        "import" => {
            // # Setup MCFLY_HISTFILE and make sure it exists.
            // export MCFLY_HISTFILE="${HISTFILE:-$HOME/.zsh_history}"
            // if [[ ! -r "${MCFLY_HISTFILE}" ]]; then
            //   echo "McFly: ${MCFLY_HISTFILE} does not exist or is not readable. Please fix this or set HISTFILE to something else before using McFly."
            //   return 1
            // fi
        }
        "delete_index" => {
            let index_path = build_index_path();
            fs::remove_dir_all(&index_path).unwrap();
        }
        _ => {
            print!("{}", HELP);
        }
    }

    std::process::exit(0);
}

fn build_schema() -> Schema {
    let mut schema_builder = Schema::builder();

    schema_builder.add_u64_field("id", FAST | INDEXED | STORED);
    schema_builder.add_u64_field("times_selected", FAST | INDEXED | STORED);
    schema_builder.add_u64_field("exit_code", INDEXED | STORED);
    schema_builder.add_text_field(
        "directory",
        TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            )
            .set_stored(),
    );
    schema_builder.add_text_field(
        "command",
        TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            )
            .set_stored(),
    );

    schema_builder.build()
}

fn build_index_path() -> std::path::PathBuf {
    Path::new(&home_dir().unwrap()).join(".fuzzy_history")
}

fn index_command(text: String) -> std::io::Result<()> {
    let parts: Vec<&str> = text.trim().split(":").collect();
    let exit_code = parts.get(0).unwrap().parse::<u64>().unwrap();
    let command_input = parts.get(1).unwrap();
    let index_path = build_index_path();

    let schema = build_schema();

    fs::create_dir_all(&index_path).unwrap();

    let directory: Box<dyn Directory> = Box::new(MmapDirectory::open(index_path).unwrap());
    let index = Index::open_or_create(directory, schema.clone()).unwrap();

    index.settings();

    let mut index_writer = index.writer(50_000_000).unwrap();

    let id_field = schema.get_field("id").unwrap();
    let times_selected_field = schema.get_field("times_selected").unwrap();
    let exit_code_field = schema.get_field("exit_code").unwrap();
    let directory_field = schema.get_field("directory").unwrap();
    let command_field = schema.get_field("command").unwrap();

    let mut command_doc = Document::default();

    command_doc.add_u64(id_field, Ulid::new().timestamp_ms());
    command_doc.add_u64(times_selected_field, 0);
    command_doc.add_u64(exit_code_field, exit_code);
    command_doc.add_text(
        directory_field,
        std::env::current_dir().unwrap().to_str().unwrap(),
    );
    command_doc.add_text(command_field, command_input);

    println!("{:#?}", "Indexing:");
    println!("{:#?}", command_doc);

    index_writer.add_document(command_doc);
    index_writer.commit().unwrap();

    Ok(())
}

fn interactive_search_command(fd_path: String, text: String) -> std::io::Result<String> {
    let results = search_command(text.clone());
    let selection = FuzzyHistorySelect::with_theme(&ColorfulTheme::default())
        .with_initial_text(text)
        .items(&results)
        .default(0)
        .interact_on_opt(fd_path, &Term::stderr());

    selection
}

fn search_command(text: String) -> Vec<String> {
    let schema = build_schema();
    let index_path = build_index_path();
    let directory: Box<dyn Directory> = Box::new(MmapDirectory::open(index_path).unwrap());
    let index = Index::open_or_create(directory, schema.clone()).unwrap();
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommit)
        .try_into()
        .unwrap();

    let searcher = reader.searcher();
    let id_field = schema.get_field("id").unwrap();
    let times_selected_field = schema.get_field("times_selected").unwrap();
    let exit_code_field = schema.get_field("exit_code").unwrap();
    let directory_field = schema.get_field("directory").unwrap();
    let command_field = schema.get_field("command").unwrap();

    let directory_term = tantivy::Term::from_field_text(
        directory_field,
        std::env::current_dir().unwrap().to_str().unwrap(),
    );
    let directory_query = TermQuery::new(directory_term, IndexRecordOption::Basic);

    let text_parts: Vec<&str> = text.split(" ").collect();
    let pattern = ["", text_parts.join(".*").as_str(), ""].join(".*");
    let command_query = RegexQuery::from_pattern(pattern.as_str(), command_field).unwrap();

    let query = BooleanQuery::new(vec![
        (Occur::Should, Box::new(directory_query)),
        (Occur::Must, Box::new(command_query)),
    ]);

    let one_month_ms: u64 = 2629800000;
    let current_ms = Ulid::new().timestamp_ms();

    let (top_docs, count) = searcher
        .search(
            &query,
            &(
                TopDocs::with_limit(10).tweak_score(move |segment_reader: &SegmentReader| {
                    let id_reader = segment_reader.fast_fields().u64(id_field).unwrap();

                    move |doc: DocId, original_score: Score| {
                        let ms_diff = current_ms - id_reader.get_val(0);

                        let decay: f64 = ms_diff as f64 / one_month_ms as f64;
                        let id_score_scaling = 1 as f64 - decay;
                        let id_score_boost = 1 as f32 * id_score_scaling as f32;

                        id_score_boost + original_score
                    }
                }),
                Count,
            ),
        )
        .unwrap();

    let mut results = vec![];

    for (_score, doc_address) in top_docs {
        let retrieved_doc = searcher.doc(doc_address).unwrap();
        let command = retrieved_doc
            .get_first(command_field)
            .unwrap()
            .as_text()
            .unwrap()
            .to_string();

        results.push(command);
        // println!("{}", schema.to_json(&retrieved_doc));
    }

    results
}

pub struct FuzzyHistorySelect<'a> {
    default: Option<usize>,
    items: Vec<String>,
    prompt: String,
    report: bool,
    clear: bool,
    highlight_matches: bool,
    max_length: Option<usize>,
    theme: &'a dyn Theme,
    /// Search string that a fuzzy search with start with.
    /// Defaults to an empty string.
    initial_text: String,
}

impl Default for FuzzyHistorySelect<'static> {
    fn default() -> Self {
        Self::new()
    }
}

impl FuzzyHistorySelect<'static> {
    /// Creates the prompt with a specific text.
    pub fn new() -> Self {
        Self::with_theme(&SimpleTheme)
    }
}

impl FuzzyHistorySelect<'_> {
    /// Sets the clear behavior of the menu.
    ///
    /// The default is to clear the menu.
    pub fn clear(&mut self, val: bool) -> &mut Self {
        self.clear = val;
        self
    }

    /// Sets a default for the menu
    pub fn default(&mut self, val: usize) -> &mut Self {
        self.default = Some(val);
        self
    }

    /// Add a single item to the fuzzy selector.
    pub fn item<T: ToString>(&mut self, item: T) -> &mut Self {
        self.items.push(item.to_string());
        self
    }

    /// Adds multiple items to the fuzzy selector.
    pub fn items<T: ToString>(&mut self, items: &[T]) -> &mut Self {
        for item in items {
            self.items.push(item.to_string());
        }
        self
    }

    pub fn set_items_from_search(&mut self, query: String) -> &mut Self {
        let new_results = search_command(query);
        self.items = new_results.iter().map(|i| i.to_string()).collect();

        // for item in items {
        //     self.items.push(item.to_string());
        // }
        self
    }

    /// Sets the search text that a fuzzy search starts with.
    pub fn with_initial_text<S: Into<String>>(&mut self, initial_text: S) -> &mut Self {
        self.initial_text = initial_text.into();
        self
    }

    /// Prefaces the menu with a prompt.
    ///
    /// When a prompt is set the system also prints out a confirmation after
    /// the fuzzy selection.
    pub fn with_prompt<S: Into<String>>(&mut self, prompt: S) -> &mut Self {
        self.prompt = prompt.into();
        self
    }

    /// Indicates whether to report the selected value after interaction.
    ///
    /// The default is to report the selection.
    pub fn report(&mut self, val: bool) -> &mut Self {
        self.report = val;
        self
    }

    /// Indicates whether to highlight matched indices
    ///
    /// The default is to highlight the indices
    pub fn highlight_matches(&mut self, val: bool) -> &mut Self {
        self.highlight_matches = val;
        self
    }

    /// Sets the maximum number of visible options.
    ///
    /// The default is the height of the terminal minus 2.
    pub fn max_length(&mut self, rows: usize) -> &mut Self {
        self.max_length = Some(rows);
        self
    }

    // /// Enables user interaction and returns the result.
    // ///
    // /// The user can select the items using 'Enter' and the index of selected item will be returned.
    // /// The dialog is rendered on stderr.
    // /// Result contains `index` of selected item if user hit 'Enter'.
    // /// This unlike [interact_opt](#method.interact_opt) does not allow to quit with 'Esc' or 'q'.
    // #[inline]
    // pub fn interact(&mut self) -> io::Result<String> {
    //     self.interact_on(&Term::stderr())
    // }

    /// Enables user interaction and returns the result.
    ///
    /// The user can select the items using 'Enter' and the index of selected item will be returned.
    /// The dialog is rendered on stderr.
    /// Result contains `Some(index)` if user hit 'Enter' or `None` if user cancelled with 'Esc' or 'q'.
    #[inline]
    pub fn interact_opt(&mut self, fd_path: String) -> io::Result<String> {
        self.interact_on_opt(fd_path, &Term::stderr())
    }

    // /// Like `interact` but allows a specific terminal to be set.
    // #[inline]
    // pub fn interact_on(&mut self, term: &Term) -> io::Result<String> {
    //     self._interact_on(term, false)?
    //         .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Quit not allowed in this case"))
    // }

    /// Like `interact` but allows a specific terminal to be set.
    #[inline]
    pub fn interact_on_opt(&mut self, fd_path: String, term: &Term) -> io::Result<String> {
        self._interact_on(fd_path, term, true)
    }

    /// Like `interact` but allows a specific terminal to be set.
    fn _interact_on(&mut self, fd_path: String, term: &Term, allow_quit: bool) -> io::Result<String> {
        // Place cursor at the end of the search term
        let mut position = self.initial_text.len();
        let mut search_term = self.initial_text.to_owned();

        let mut render = TermThemeRenderer::new(term, self.theme);
        let mut sel = self.default;

        let mut size_vec = Vec::new();
        for items in self.items.iter().as_slice() {
            let size = &items.len();
            size_vec.push(*size);
        }

        // Fuzzy matcher
        let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();

        // Subtract -2 because we need space to render the prompt.
        let visible_term_rows = (term.size().0 as usize).max(3) - 2;
        let visible_term_rows = self
            .max_length
            .unwrap_or(visible_term_rows)
            .min(visible_term_rows);
        // Variable used to determine if we need to scroll through the list.
        let mut starting_row = 0;

        term.hide_cursor()?;
        // term.show_cursor()?;

        loop {
            render.clear()?;
            // println!("{:#?}", "loop");
            render.fuzzy_select_prompt(self.prompt.as_str(), &search_term, position)?;

            for (idx, item) in self
                .items
                .iter()
                .enumerate()
                .skip(starting_row)
                .take(visible_term_rows)
            {
                render.fuzzy_select_prompt_item(
                    item,
                    Some(idx) == sel,
                    self.highlight_matches,
                    &matcher,
                    &search_term,
                )?;
            }
            term.flush()?;

            match (term.read_raw_key(fd_path.clone())?, sel) {
                (Key::Escape, _) if allow_quit => {
                    // println!("{:#?}", "Escape");
                    if self.clear {
                        render.clear()?;
                        term.flush()?;
                    }
                    term.show_cursor()?;
                    return Err(io::Error::new(io::ErrorKind::Interrupted, ""));
                }
                (Key::ArrowUp | Key::BackTab, _) if !self.items.is_empty() => {
                    // println!("{:#?}", "ArrowUp");
                    if sel == Some(0) {
                        starting_row = self.items.len().max(visible_term_rows) - visible_term_rows;
                    } else if sel == Some(starting_row) {
                        starting_row -= 1;
                    }
                    sel = match sel {
                        None => Some(self.items.len() - 1),
                        Some(sel) => Some(
                            ((sel as i64 - 1 + self.items.len() as i64) % (self.items.len() as i64))
                                as usize,
                        ),
                    };
                    term.flush()?;
                }
                (Key::ArrowDown | Key::Tab, _) if !self.items.is_empty() => {
                    // println!("{:#?}", "ArrowDown");
                    sel = match sel {
                        None => Some(0),
                        Some(sel) => Some((sel as u64 + 1).rem(self.items.len() as u64) as usize),
                    };
                    if sel == Some(visible_term_rows + starting_row) {
                        starting_row += 1;
                    } else if sel == Some(0) {
                        starting_row = 0;
                    }
                    term.flush()?;
                }
                (Key::ArrowLeft, _) if position > 0 => {
                    // println!("{:#?}", "ArrowLeft");
                    position -= 1;
                    term.flush()?;
                }
                (Key::ArrowRight, _) if position < search_term.len() => {
                    // println!("{:#?}", "ArrowRight");
                    position += 1;
                    term.flush()?;
                }
                (Key::Enter, Some(sel)) if !self.items.is_empty() => {
                    // println!("{:#?}", "Enter");
                    if self.clear {
                        render.clear()?;
                    }

                    term.show_cursor()?;
                    return Ok(self.items[sel].clone());
                }
                (Key::Backspace, _) if position > 0 => {
                    // println!("{:#?}", "Backspace");
                    if search_term.is_char_boundary(position) {
                        position -= 1;
                        search_term.remove(position);

                        self.set_items_from_search(search_term.clone());
                        term.flush()?;
                    }
                }
                (Key::Char(chr), _) if !chr.is_ascii_control() => {
                    // println!("char: {:#?}", chr);

                    if search_term.is_char_boundary(position) {
                        search_term.insert(position, chr);
                        position += 1;

                        self.set_items_from_search(search_term.clone());
                        term.flush()?;

                        sel = Some(0);
                        starting_row = 0;


                        // match chr.to_string().as_str() {
                        //     "A" => {
                        //         // println!("{:#?}", "ArrowUp");
                        //         if sel == Some(0) {
                        //             starting_row = self.items.len().max(visible_term_rows) - visible_term_rows;
                        //         } else if sel == Some(starting_row) {
                        //             starting_row -= 1;
                        //         }
                        //         sel = match sel {
                        //             None => Some(self.items.len() - 1),
                        //             Some(sel) => Some(
                        //                 ((sel as i64 - 1 + self.items.len() as i64) % (self.items.len() as i64))
                        //                     as usize,
                        //             ),
                        //         };
                        //         term.flush()?;

                        //     },
                        //     "B" => {
                        //         // println!("{:#?}", "ArrowDown");
                        //         sel = match sel {
                        //             None => Some(0),
                        //             Some(sel) => Some((sel as u64 + 1).rem(self.items.len() as u64) as usize),
                        //         };
                        //         if sel == Some(visible_term_rows + starting_row) {
                        //             starting_row += 1;
                        //         } else if sel == Some(0) {
                        //             starting_row = 0;
                        //         }
                        //         term.flush()?;
                        //     },
                        //     "C" => {
                        //         position += 1;
                        //         term.flush()?;
                        //     },
                        //     "D" => {
                        //         // println!("{:#?}", "ArrowLeft");
                        //         position -= 1;
                        //         term.flush()?;
                        //     },
                        //     _ => {
                        //         search_term.insert(position, chr);
                        //         position += 1;

                        //         self.set_items_from_search(search_term.clone());
                        //         term.flush()?;

                        //         sel = Some(0);
                        //         starting_row = 0;
                        //     },
                        // }
                    }
                }

                _ => {
                    // println!("{:#?}", "Default");
                }
            }

            render.clear_preserve_prompt(&size_vec)?;
        }
    }
}

impl<'a> FuzzyHistorySelect<'a> {
    /// Same as `new` but with a specific theme.
    pub fn with_theme(theme: &'a dyn Theme) -> Self {
        Self {
            default: None,
            items: vec![],
            prompt: "".into(),
            report: true,
            clear: true,
            highlight_matches: true,
            max_length: None,
            theme,
            initial_text: "".into(),
        }
    }
}

/// Helper struct to conveniently render a theme of a term.
pub(crate) struct TermThemeRenderer<'a> {
    term: &'a Term,
    theme: &'a dyn Theme,
    height: usize,
    prompt_height: usize,
    prompts_reset_height: bool,
}

impl<'a> TermThemeRenderer<'a> {
    pub fn new(term: &'a Term, theme: &'a dyn Theme) -> TermThemeRenderer<'a> {
        TermThemeRenderer {
            term,
            theme,
            height: 0,
            prompt_height: 0,
            prompts_reset_height: true,
        }
    }

    #[cfg(feature = "password")]
    pub fn set_prompts_reset_height(&mut self, val: bool) {
        self.prompts_reset_height = val;
    }

    #[cfg(feature = "password")]
    pub fn term(&self) -> &Term {
        self.term
    }

    pub fn add_line(&mut self) {
        self.height += 1;
    }

    fn write_formatted_str<
        F: FnOnce(&mut TermThemeRenderer, &mut dyn fmt::Write) -> fmt::Result,
    >(
        &mut self,
        f: F,
    ) -> io::Result<usize> {
        let mut buf = String::new();
        f(self, &mut buf).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        self.height += buf.chars().filter(|&x| x == '\n').count();
        self.term.write_str(&buf)?;
        Ok(measure_text_width(&buf))
    }

    fn write_formatted_line<
        F: FnOnce(&mut TermThemeRenderer, &mut dyn fmt::Write) -> fmt::Result,
    >(
        &mut self,
        f: F,
    ) -> io::Result<()> {
        let mut buf = String::new();
        f(self, &mut buf).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        self.height += buf.chars().filter(|&x| x == '\n').count() + 1;
        self.term.write_line(&buf)
    }

    fn write_formatted_prompt<
        F: FnOnce(&mut TermThemeRenderer, &mut dyn fmt::Write) -> fmt::Result,
    >(
        &mut self,
        f: F,
    ) -> io::Result<()> {
        self.write_formatted_line(f)?;
        if self.prompts_reset_height {
            self.prompt_height = self.height;
            self.height = 0;
        }
        Ok(())
    }

    fn write_paging_info(buf: &mut dyn fmt::Write, paging_info: (usize, usize)) -> fmt::Result {
        write!(buf, " [Page {}/{}] ", paging_info.0, paging_info.1)
    }

    pub fn error(&mut self, err: &str) -> io::Result<()> {
        self.write_formatted_line(|this, buf| this.theme.format_error(buf, err))
    }

    pub fn confirm_prompt(&mut self, prompt: &str, default: Option<bool>) -> io::Result<usize> {
        self.write_formatted_str(|this, buf| this.theme.format_confirm_prompt(buf, prompt, default))
    }

    pub fn confirm_prompt_selection(&mut self, prompt: &str, sel: Option<bool>) -> io::Result<()> {
        self.write_formatted_prompt(|this, buf| {
            this.theme.format_confirm_prompt_selection(buf, prompt, sel)
        })
    }

    pub fn fuzzy_select_prompt(
        &mut self,
        prompt: &str,
        search_term: &str,
        cursor_pos: usize,
    ) -> io::Result<()> {
        self.write_formatted_prompt(|this, buf| {
            this.theme
                .format_fuzzy_select_prompt(buf, prompt, search_term, cursor_pos)
        })
    }

    pub fn input_prompt(&mut self, prompt: &str, default: Option<&str>) -> io::Result<usize> {
        self.write_formatted_str(|this, buf| this.theme.format_input_prompt(buf, prompt, default))
    }

    pub fn input_prompt_selection(&mut self, prompt: &str, sel: &str) -> io::Result<()> {
        self.write_formatted_prompt(|this, buf| {
            this.theme.format_input_prompt_selection(buf, prompt, sel)
        })
    }

    #[cfg(feature = "password")]
    pub fn password_prompt(&mut self, prompt: &str) -> io::Result<usize> {
        self.write_formatted_str(|this, buf| {
            write!(buf, "\r")?;
            this.theme.format_password_prompt(buf, prompt)
        })
    }

    #[cfg(feature = "password")]
    pub fn password_prompt_selection(&mut self, prompt: &str) -> io::Result<()> {
        self.write_formatted_prompt(|this, buf| {
            this.theme.format_password_prompt_selection(buf, prompt)
        })
    }

    pub fn select_prompt(
        &mut self,
        prompt: &str,
        paging_info: Option<(usize, usize)>,
    ) -> io::Result<()> {
        self.write_formatted_prompt(|this, buf| {
            this.theme.format_select_prompt(buf, prompt)?;

            if let Some(paging_info) = paging_info {
                TermThemeRenderer::write_paging_info(buf, paging_info)?;
            }

            Ok(())
        })
    }

    pub fn select_prompt_selection(&mut self, prompt: &str, sel: &str) -> io::Result<()> {
        self.write_formatted_prompt(|this, buf| {
            this.theme.format_select_prompt_selection(buf, prompt, sel)
        })
    }

    pub fn select_prompt_item(&mut self, text: &str, active: bool) -> io::Result<()> {
        self.write_formatted_line(|this, buf| {
            this.theme.format_select_prompt_item(buf, text, active)
        })
    }

    pub fn fuzzy_select_prompt_item(
        &mut self,
        text: &str,
        active: bool,
        highlight: bool,
        matcher: &fuzzy_matcher::skim::SkimMatcherV2,
        search_term: &str,
    ) -> io::Result<()> {
        self.write_formatted_line(|this, buf| {
            this.theme.format_fuzzy_select_prompt_item(
                buf,
                text,
                active,
                highlight,
                matcher,
                search_term,
            )
        })
    }

    pub fn multi_select_prompt(
        &mut self,
        prompt: &str,
        paging_info: Option<(usize, usize)>,
    ) -> io::Result<()> {
        self.write_formatted_prompt(|this, buf| {
            this.theme.format_multi_select_prompt(buf, prompt)?;

            if let Some(paging_info) = paging_info {
                TermThemeRenderer::write_paging_info(buf, paging_info)?;
            }

            Ok(())
        })
    }

    pub fn multi_select_prompt_selection(&mut self, prompt: &str, sel: &[&str]) -> io::Result<()> {
        self.write_formatted_prompt(|this, buf| {
            this.theme
                .format_multi_select_prompt_selection(buf, prompt, sel)
        })
    }

    pub fn multi_select_prompt_item(
        &mut self,
        text: &str,
        checked: bool,
        active: bool,
    ) -> io::Result<()> {
        self.write_formatted_line(|this, buf| {
            this.theme
                .format_multi_select_prompt_item(buf, text, checked, active)
        })
    }

    pub fn sort_prompt(
        &mut self,
        prompt: &str,
        paging_info: Option<(usize, usize)>,
    ) -> io::Result<()> {
        self.write_formatted_prompt(|this, buf| {
            this.theme.format_sort_prompt(buf, prompt)?;

            if let Some(paging_info) = paging_info {
                TermThemeRenderer::write_paging_info(buf, paging_info)?;
            }

            Ok(())
        })
    }

    pub fn sort_prompt_selection(&mut self, prompt: &str, sel: &[&str]) -> io::Result<()> {
        self.write_formatted_prompt(|this, buf| {
            this.theme.format_sort_prompt_selection(buf, prompt, sel)
        })
    }

    pub fn sort_prompt_item(&mut self, text: &str, picked: bool, active: bool) -> io::Result<()> {
        self.write_formatted_line(|this, buf| {
            this.theme
                .format_sort_prompt_item(buf, text, picked, active)
        })
    }

    pub fn clear(&mut self) -> io::Result<()> {
        self.term
            .clear_last_lines(self.height + self.prompt_height)?;
        self.height = 0;
        self.prompt_height = 0;
        Ok(())
    }

    pub fn clear_preserve_prompt(&mut self, size_vec: &[usize]) -> io::Result<()> {
        let mut new_height = self.height;
        let prefix_width = 2;
        //Check each item size, increment on finding an overflow
        for size in size_vec {
            if *size > self.term.size().1 as usize {
                new_height += (((*size as f64 + prefix_width as f64) / self.term.size().1 as f64)
                    .ceil()) as usize
                    - 1;
            }
        }

        self.term.clear_last_lines(new_height)?;
        self.height = 0;
        Ok(())
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_str() {
//         let selections = &[
//             "Ice Cream",
//             "Vanilla Cupcake",
//             "Chocolate Muffin",
//             "A Pile of sweet, sweet mustard",
//         ];

//         assert_eq!(
//             Select::new().default(0).items(&selections[..]).items,
//             selections
//         );
//     }

//     #[test]
//     fn test_string() {
//         let selections = vec!["a".to_string(), "b".to_string()];

//         assert_eq!(
//             Select::new().default(0).items(&selections[..]).items,
//             selections
//         );
//     }

//     #[test]
//     fn test_ref_str() {
//         let a = "a";
//         let b = "b";

//         let selections = &[a, b];

//         assert_eq!(
//             Select::new().default(0).items(&selections[..]).items,
//             selections
//         );
//     }
// }
