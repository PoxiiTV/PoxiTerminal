//! Búsqueda en el historial (Ctrl+F): barra flotante sobre el terminal y
//! resaltado de coincidencias.
//!
//! El motor es el de `nebula_terminal::term::search` (el mismo que usaba el
//! shell antiguo); aquí solo vive el estado de la barra y la conversión de
//! coincidencias a tramos de celdas visibles. El texto se busca **literal**:
//! el usuario escribe `error (conexión` y espera encontrar eso, no un regex
//! inválido.

use std::cell::RefCell;

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Styled as _, Subscription, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _};
use nebula_terminal::grid::Dimensions as _;
use nebula_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use nebula_terminal::term::Term;
use nebula_terminal::term::search::{Match, RegexIter, RegexSearch};

use super::TerminalView;

/// Más allá de este número de coincidencias el contador muestra "999+": contar
/// todo el historial en cada tecla no aporta nada y cuesta en historiales enormes.
const MAX_COUNTED: usize = 999;

pub(in crate::gpui_shell::terminal) struct ScrollbackSearch {
    pub(super) input: Entity<InputState>,
    pub(in crate::gpui_shell::terminal) matcher: SearchMatcher,
    _subscription: Subscription,
}

/// Núcleo de la búsqueda, sin GPUI: patrón, coincidencia enfocada y contador.
#[derive(Default)]
pub(in crate::gpui_shell::terminal) struct SearchMatcher {
    /// El DFA necesita `&mut` para sus cachés, pero el elemento de pintado solo
    /// tiene acceso de lectura a la vista.
    regex: RefCell<Option<RegexSearch>>,
    /// Coincidencia enfocada en líneas **absolutas** (no cambian cuando entra
    /// salida nueva y el historial se desplaza).
    focused: Option<AbsMatch>,
    /// (posición de la coincidencia enfocada empezando en 1, total contado).
    position: Option<(usize, usize)>,
}

/// Coincidencia en líneas absolutas: (línea, columna) de inicio y de fin.
type AbsMatch = ((i64, usize), (i64, usize));

/// Línea absoluta de la línea 0 de la rejilla: todo lo que ya salió del
/// historial más lo que queda en él.
fn anchor<T>(term: &Term<T>) -> i64 {
    (term.grid().scrolled_out() + term.history_size()) as i64
}

fn to_abs<T>(m: &Match, term: &Term<T>) -> AbsMatch {
    let base = anchor(term);
    (
        (base + i64::from(m.start().line.0), m.start().column.0),
        (base + i64::from(m.end().line.0), m.end().column.0),
    )
}

/// Vuelve a coordenadas de la rejilla; None si ya salió del historial.
fn from_abs<T>(m: &AbsMatch, term: &Term<T>) -> Option<Match> {
    let base = anchor(term);
    let line = |abs: i64| i32::try_from(abs - base).ok().map(Line);
    let (start, end) = (line(m.0.0)?, line(m.1.0)?);
    if start < term.topmost_line() || end > term.bottommost_line() {
        return None;
    }
    Some(Point::new(start, Column(m.0.1))..=Point::new(end, Column(m.1.1)))
}

/// Una coincidencia recortada a una fila visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::gpui_shell::terminal) struct MatchRun {
    pub row: u16,
    pub start: u16,
    pub end: u16,
    pub focused: bool,
}

/// Escapa los metacaracteres del regex para buscar el texto tal cual.
fn literal_pattern(query: &str) -> String {
    let mut pattern = String::with_capacity(query.len());
    for c in query.chars() {
        if matches!(
            c,
            '\\' | '.'
                | '+'
                | '*'
                | '?'
                | '('
                | ')'
                | '|'
                | '['
                | ']'
                | '{'
                | '}'
                | '^'
                | '$'
                | '#'
                | '&'
                | '-'
                | '~'
        ) {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern
}

/// Convierte coincidencias (coordenadas de rejilla) en tramos por fila del
/// viewport que empieza en `origin`. Las que caen fuera se descartan y las que
/// ocupan varias líneas se parten.
fn match_runs(
    matches: &[Match],
    focused: Option<&Match>,
    origin: Line,
    rows: usize,
    cols: usize,
) -> Vec<MatchRun> {
    let mut runs = Vec::new();
    for m in matches {
        let is_focused = focused == Some(m);
        let (start, end) = (*m.start(), *m.end());
        for line in start.line.0..=end.line.0 {
            let row = line - origin.0;
            if row < 0 || row as usize >= rows {
                continue;
            }
            let from = if line == start.line.0 { start.column.0 } else { 0 };
            let to = if line == end.line.0 { end.column.0 + 1 } else { cols };
            let to = to.min(cols);
            if from >= to {
                continue;
            }
            runs.push(MatchRun {
                row: row as u16,
                start: from as u16,
                end: to as u16,
                focused: is_focused,
            });
        }
    }
    runs
}

impl SearchMatcher {
    fn set_query(&mut self, query: &str) {
        self.focused = None;
        self.position = None;
        *self.regex.borrow_mut() =
            if query.is_empty() { None } else { RegexSearch::new(&literal_pattern(query)).ok() };
    }

    /// Salta a la coincidencia siguiente o anterior (con vuelta al principio),
    /// desplaza el historial hasta ella y recalcula el contador.
    fn step<T: nebula_terminal::event::EventListener>(
        &mut self,
        term: &mut Term<T>,
        direction: Direction,
    ) {
        let found = {
            let mut regex = self.regex.borrow_mut();
            let Some(regex) = regex.as_mut() else { return };
            let focused = self.focused.as_ref().and_then(|m| from_abs(m, term));
            let origin = match (&focused, direction) {
                (Some(m), Direction::Right) => m.end().add(&*term, Boundary::Grid, 1),
                (Some(m), Direction::Left) => m.start().sub(&*term, Boundary::Grid, 1),
                (None, Direction::Right) => Point::new(term.topmost_line(), Column(0)),
                (None, Direction::Left) => Point::new(term.bottommost_line(), term.last_column()),
            };
            let side = if direction == Direction::Right { Side::Left } else { Side::Right };
            term.search_next(regex, origin, direction, side, None)
        };
        if let Some(m) = &found {
            term.scroll_to_point(*m.start());
        }
        self.focused = found.as_ref().map(|m| to_abs(m, term));
        self.position = self.count_and_position(term);
    }

    /// Tramos a resaltar en el viewport actual. Se llama dentro del mismo
    /// bloqueo del `Term` que el snapshot de pintado.
    pub(in crate::gpui_shell::terminal) fn visible_runs<T>(
        &self,
        term: &Term<T>,
        origin: Line,
        rows: usize,
        cols: usize,
    ) -> Vec<MatchRun> {
        let mut regex = self.regex.borrow_mut();
        let Some(regex) = regex.as_mut() else { return Vec::new() };
        let top = origin;
        let bottom = origin + (rows.saturating_sub(1)) as i32;
        let start = term.line_search_left(Point::new(top.max(term.topmost_line()), Column(0)));
        let end = term.line_search_right(Point::new(bottom.min(term.bottommost_line()), Column(0)));
        let matches: Vec<Match> = RegexIter::new(start, end, Direction::Right, term, regex)
            .skip_while(|m| m.end().line < top)
            .take_while(|m| m.start().line <= bottom)
            .collect();
        let focused = self.focused.as_ref().and_then(|m| from_abs(m, term));
        match_runs(&matches, focused.as_ref(), origin, rows, cols)
    }

    fn count_and_position<T>(&self, term: &Term<T>) -> Option<(usize, usize)> {
        let mut regex = self.regex.borrow_mut();
        let regex = regex.as_mut()?;
        let start = Point::new(term.topmost_line(), Column(0));
        let end = Point::new(term.bottommost_line(), term.last_column());
        let mut total = 0;
        let mut position = 0;
        for m in RegexIter::new(start, end, Direction::Right, term, regex) {
            total += 1;
            if Some(to_abs(&m, term)) == self.focused {
                position = total;
            }
            if total > MAX_COUNTED {
                break;
            }
        }
        Some((position, total))
    }
}

impl TerminalView {
    /// Ctrl+F. Si la barra ya está abierta solo le devuelve el foco.
    pub(super) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search) = &self.search {
            let input = search.input.clone();
            input.update(cx, |input, cx| input.focus(window, cx));
            return;
        }
        let language = super::ui_language();
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.pick("在历史中搜索", "Search history"))
        });
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| match event {
                InputEvent::Change => this.search_query_changed(cx),
                InputEvent::PressEnter { shift, .. } => {
                    this.search_step(if *shift { Direction::Left } else { Direction::Right }, cx)
                },
                _ => {},
            });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.search = Some(ScrollbackSearch {
            input,
            matcher: SearchMatcher::default(),
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    fn search_query_changed(&mut self, cx: &mut Context<Self>) {
        let Some(search) = &mut self.search else { return };
        let query = search.input.read(cx).value().to_string();
        search.matcher.set_query(&query);
        // Al escribir se busca hacia arriba desde el final: lo que se busca
        // casi siempre es salida reciente.
        self.search_step(Direction::Left, cx);
    }

    fn search_step(&mut self, direction: Direction, cx: &mut Context<Self>) {
        let (Some(session), Some(search)) = (&self.session, &mut self.search) else { return };
        search.matcher.step(&mut session.term.lock(), direction);
        cx.notify();
    }

    /// Esc cierra; F3/Mayús+F3 también saltan. Va en la fase de captura para
    /// adelantarse al campo de texto.
    fn search_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        match ks.key.as_str() {
            "escape" => {
                self.close_search(window, cx);
                cx.stop_propagation();
            },
            "f3" => {
                let direction = if ks.modifiers.shift { Direction::Left } else { Direction::Right };
                self.search_step(direction, cx);
                cx.stop_propagation();
            },
            _ => {},
        }
    }

    pub(super) fn render_search_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let search = self.search.as_ref()?;
        let language = super::ui_language();
        let theme = cx.theme();
        let has_query = !search.input.read(cx).value().is_empty();
        let status = match search.matcher.position {
            _ if !has_query => String::new(),
            Some((_, 0)) | None => language.pick("无结果", "No results").to_owned(),
            Some((position, total)) => {
                let total =
                    if total > MAX_COUNTED { format!("{MAX_COUNTED}+") } else { total.to_string() };
                language
                    .pick("{current} / {total}", "{current} of {total}")
                    .replace("{current}", &position.to_string())
                    .replace("{total}", &total)
            },
        };
        let no_results = has_query && matches!(search.matcher.position, Some((_, 0)) | None);
        let icon_button = |id: &'static str, icon: IconName, tooltip: &'static str| {
            Button::new(id).ghost().xsmall().icon(Icon::new(icon)).tooltip(tooltip)
        };
        Some(
            div()
                .id("scrollback-search")
                .absolute()
                .top_2()
                .right_3()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover.opacity(0.92))
                .shadow_lg()
                .capture_key_down(cx.listener(Self::search_key))
                // Los clics en la barra no deben llegar al terminal (selección).
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div().w(px(240.0)).child(
                        Input::new(&search.input)
                            .appearance(false)
                            .small()
                            .prefix(Icon::new(IconName::Search).text_color(theme.muted_foreground)),
                    ),
                )
                .child(
                    div()
                        .min_w(px(64.0))
                        .text_xs()
                        .text_color(if no_results { theme.danger } else { theme.muted_foreground })
                        .child(status),
                )
                .child(
                    icon_button(
                        "search-prev",
                        IconName::ChevronUp,
                        language.pick("上一个 (Shift+Enter)", "Previous (Shift+Enter)"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.search_step(Direction::Left, cx))),
                )
                .child(
                    icon_button(
                        "search-next",
                        IconName::ChevronDown,
                        language.pick("下一个 (Enter)", "Next (Enter)"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.search_step(Direction::Right, cx)
                    })),
                )
                .child(
                    icon_button("search-close", IconName::Close, language.pick("关闭 (Esc)", "Close (Esc)"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx))),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use nebula_terminal::event::VoidListener;
    use nebula_terminal::term::Config;
    use nebula_terminal::term::test::TermSize;
    use nebula_terminal::vte::ansi;

    use super::*;

    fn point(line: i32, column: usize) -> Point {
        Point::new(Line(line), Column(column))
    }

    fn term_with(text: &str) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &TermSize::new(20, 4), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut term, text.as_bytes());
        term
    }

    #[test]
    fn step_finds_matches_through_history_counts_and_wraps() {
        // 8 líneas en una pantalla de 4: dos coincidencias quedan en el historial.
        let mut term = term_with(
            "uno error(x)\r\ndos\r\ntres error(x)\r\ncuatro\r\ncinco\r\nseis\r\nsiete error(x)\r\nocho",
        );
        let mut matcher = SearchMatcher::default();
        matcher.set_query("error(x)");

        // Al escribir se busca hacia arriba: la más reciente es la 3 de 3.
        matcher.step(&mut term, Direction::Left);
        assert_eq!(matcher.position, Some((3, 3)));
        matcher.step(&mut term, Direction::Left);
        assert_eq!(matcher.position, Some((2, 3)));
        // La 2 está en el historial: el viewport tiene que haber subido hasta ella.
        assert!(term.grid().display_offset() > 0);
        let origin = term.viewport_origin_for(4);
        let runs = matcher.visible_runs(&term, origin, 4, 20);
        assert!(runs.iter().any(|run| run.focused && run.start == 5 && run.end == 13), "{runs:?}");
        matcher.step(&mut term, Direction::Left);
        assert_eq!(matcher.position, Some((1, 3)));
        // Vuelta: tras la primera, hacia arriba otra vez se va a la última.
        matcher.step(&mut term, Direction::Left);
        assert_eq!(matcher.position, Some((3, 3)));
        matcher.step(&mut term, Direction::Right);
        assert_eq!(matcher.position, Some((1, 3)));
    }

    #[test]
    fn focused_match_survives_new_output() {
        let mut term = term_with(
            "uno error(x)
dos
tres",
        );
        let mut matcher = SearchMatcher::default();
        matcher.set_query("error(x)");
        matcher.step(&mut term, Direction::Left);
        assert_eq!(matcher.position, Some((1, 1)));
        // Entra salida nueva (como una IA escribiendo): la enfocada sigue siendo la misma.
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(
            &mut term,
            b"
cuatro
cinco
seis
siete error(x)",
        );
        let focused = from_abs(matcher.focused.as_ref().unwrap(), &term).unwrap();
        let text: String = (focused.start().column.0..=focused.end().column.0)
            .map(|col| term.grid()[focused.start().line][Column(col)].c)
            .collect();
        assert_eq!(text, "error(x)");
        assert_eq!(focused.start().column.0, 4, "sigue apuntando a «uno error(x)»");
        matcher.step(&mut term, Direction::Right);
        assert_eq!(matcher.position, Some((2, 2)));
    }

    #[test]
    fn empty_or_missing_query_has_no_results() {
        let mut term = term_with("hola mundo");
        let mut matcher = SearchMatcher::default();
        matcher.set_query("adios");
        matcher.step(&mut term, Direction::Left);
        assert_eq!(matcher.position, Some((0, 0)));
        matcher.set_query("");
        matcher.step(&mut term, Direction::Left);
        assert_eq!(matcher.position, None);
        assert!(matcher.visible_runs(&term, Line(0), 4, 20).is_empty());
    }

    #[test]
    fn literal_pattern_escapes_regex_metacharacters() {
        assert_eq!(literal_pattern("error (conexión)"), r"error \(conexión\)");
        assert_eq!(literal_pattern("a.b*c?"), r"a\.b\*c\?");
        assert_eq!(literal_pattern("C:\\Users"), r"C:\\Users");
        assert_eq!(literal_pattern("plain text"), "plain text");
        assert!(RegexSearch::new(&literal_pattern("[unclosed ( { ^$|")).is_ok());
    }

    #[test]
    fn match_runs_clip_to_viewport_and_split_lines() {
        let single = point(-3, 2)..=point(-3, 5);
        let wrapped = point(-1, 8)..=point(0, 1);
        let outside = point(-40, 0)..=point(-40, 3);
        let matches = [single.clone(), wrapped, outside];
        let runs = match_runs(&matches, Some(&single), Line(-4), 5, 10);
        assert_eq!(
            runs,
            vec![
                MatchRun { row: 1, start: 2, end: 6, focused: true },
                MatchRun { row: 3, start: 8, end: 10, focused: false },
                MatchRun { row: 4, start: 0, end: 2, focused: false },
            ]
        );
    }
}
