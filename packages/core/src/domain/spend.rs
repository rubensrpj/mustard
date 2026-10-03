//! O gasto de cada dia: a conta pura, sem disco e sem relógio. O Mustard conta
//! cada dia fechado pelas conversas do Claude Code, um projeto por vez, e
//! guarda a linha de cada dia num arquivo da máquina (o [`Ledger`]); a página
//! do gasto lê essas linhas de um banco de dados. O dia de hoje é aberto: entra
//! no resumo e na página como parcial, e nunca no arquivo dos dias fechados.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Offset, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use crate::platform::i18n::{translate, Locale};

/// O formato de um dia: `2026-10-01`.
const DAY_FORMAT: &str = "%Y-%m-%d";

/// O fuso em que o dia é contado, -03:00 em toda máquina: o dia de uma
/// conversa não muda conforme o relógio de quem roda a conta.
fn zone() -> FixedOffset {
    FixedOffset::west_opt(3 * 3600).unwrap_or_else(|| Utc.fix())
}

/// O dia em que o instante `at` cai, no fuso do gasto.
#[must_use]
pub fn day_of(at: DateTime<Utc>) -> String {
    at.with_timezone(&zone()).format(DAY_FORMAT).to_string()
}

/// O dia de um carimbo em RFC 3339, com o deslocamento que ele trouxer;
/// `None` quando o texto não é um carimbo.
#[must_use]
pub fn day_of_stamp(stamp: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(stamp).ok().map(|at| day_of(at.with_timezone(&Utc)))
}

/// O dia seguinte a `day`; `None` quando `day` não é um dia.
#[must_use]
pub fn next_day(day: &str) -> Option<String> {
    let date = NaiveDate::parse_from_str(day, DAY_FORMAT).ok()?;
    date.succ_opt().map(|next| next.format(DAY_FORMAT).to_string())
}

/// O dia anterior a `day`; `None` quando `day` não é um dia.
#[must_use]
pub fn previous_day(day: &str) -> Option<String> {
    let date = NaiveDate::parse_from_str(day, DAY_FORMAT).ok()?;
    date.pred_opt().map(|before| before.format(DAY_FORMAT).to_string())
}

/// O instante em que `day` começa no fuso do gasto, em UTC: uma conversa que
/// não mudou desde então não tem linha do dia nem de depois dele.
#[must_use]
pub fn day_start(day: &str) -> Option<DateTime<Utc>> {
    let midnight = NaiveDate::parse_from_str(day, DAY_FORMAT).ok()?.and_hms_opt(0, 0, 0)?;
    zone().from_local_datetime(&midnight).single().map(|at| at.with_timezone(&Utc))
}

/// Se o uso da ferramenta `name` é uma procura de código: `Grep`, `Glob`, o
/// agente `Explore` (`subagent` é o tipo de agente de um `Agent`, `Task` nas
/// conversas mais antigas) e o `Bash` cujo comando começa com `rg`, `grep` ou
/// `find`, com ou sem o caminho do programa. `Read` nunca é procura.
#[must_use]
pub fn is_code_search(name: &str, command: Option<&str>, subagent: Option<&str>) -> bool {
    match name {
        "Grep" | "Glob" => true,
        "Agent" | "Task" => subagent == Some("Explore"),
        "Bash" => {
            let first = command.unwrap_or_default().split_whitespace().next().unwrap_or_default();
            matches!(first.rsplit(['/', '\\']).next().unwrap_or(first), "rg" | "grep" | "find")
        }
        _ => false,
    }
}

/// A linha de um dia de um projeto.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayRow {
    /// O dia, `AAAA-MM-DD`, no fuso do gasto.
    pub day: String,
    /// O nome da pasta do projeto, lido do `cwd` gravado na conversa.
    pub project: String,
    /// Entrada, criação e leitura de cache e saída, cada resposta uma vez.
    #[serde(default)]
    pub tokens: u64,
    /// Cada uso de ferramenta.
    #[serde(default)]
    pub actions: u64,
    /// As ações que são procuras de código ([`is_code_search`]).
    #[serde(default)]
    pub code_searches: u64,
    /// Os tokens e o custo (em milionésimos de dólar) do Jev nas chamadas
    /// `word search` do dia.
    #[serde(default)]
    pub jev_tokens: u64,
    #[serde(default)]
    pub jev_cost_micro_usd: u64,
    /// A linha é do dia aberto (hoje): a página a marca como parcial.
    #[serde(default)]
    pub partial: bool,
}

impl DayRow {
    /// O nome do documento da linha no banco da página: o dia e o projeto em
    /// minúsculas, números e hífens.
    #[must_use]
    pub fn doc_id(&self) -> String {
        let mut slug = String::new();
        for c in self.project.chars() {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
            } else if !slug.ends_with('-') && !slug.is_empty() {
                slug.push('-');
            }
        }
        let slug = slug.trim_end_matches('-');
        format!("{}-{}", self.day, if slug.is_empty() { "project" } else { slug })
    }
}

/// O mínimo de ações de um dia da máquina inteira para ele entrar numa média.
pub const MIN_ACTIONS: u64 = 100;

/// Os dias da média curta e da longa.
const SHORT_WINDOW: usize = 3;
const LONG_WINDOW: usize = 7;

/// O consumo de um dia da máquina inteira: os projetos somados.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DayTotal {
    pub day: String,
    pub tokens: u64,
    pub actions: u64,
    pub jev_tokens: u64,
    pub jev_cost_micro_usd: u64,
}

/// A média de tokens por dia e quantos dias entraram nela.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Average {
    pub tokens: u64,
    pub days: u64,
}

/// A previsão do gasto do Jev no mês, em milionésimos de dólar e em tokens: o
/// gasto dos dias fechados do mês mais a média por dia dos últimos dias com
/// busca vezes os dias que faltam (hoje incluído).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Forecast {
    pub spent_micro_usd: u64,
    pub spent_tokens: u64,
    pub daily_micro_usd: u64,
    pub daily_tokens: u64,
    pub days_left: u64,
    pub micro_usd: u64,
    pub tokens: u64,
}

/// O resumo do topo da página: o consumo da máquina inteira.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// Hoje até agora, o dia aberto, e ontem, o último dia fechado.
    pub today: DayTotal,
    pub yesterday: DayTotal,
    /// As médias dos últimos três e sete dias fechados e dos do mês.
    pub last_3: Average,
    pub last_7: Average,
    pub month: Average,
    pub forecast: Forecast,
    /// O mínimo de ações que a página diz.
    pub min_actions: u64,
}

/// A coleção do banco da página onde o resumo mora, e o nome do documento.
pub const SUMMARY_COLLECTION: &str = "summary";
pub const SUMMARY_DOC: &str = "current";

/// Os projetos somados, um total por dia, do mais velho ao mais novo.
fn totals<'a>(rows: impl IntoIterator<Item = &'a DayRow>) -> Vec<DayTotal> {
    let mut days: BTreeMap<&str, DayTotal> = BTreeMap::new();
    for row in rows {
        let total = days.entry(row.day.as_str()).or_insert_with(|| DayTotal { day: row.day.clone(), ..DayTotal::default() });
        total.tokens = total.tokens.saturating_add(row.tokens);
        total.actions = total.actions.saturating_add(row.actions);
        total.jev_tokens = total.jev_tokens.saturating_add(row.jev_tokens);
        total.jev_cost_micro_usd = total.jev_cost_micro_usd.saturating_add(row.jev_cost_micro_usd);
    }
    days.into_values().collect()
}

/// A média de tokens por dia dos `days` mais novos, até `window` deles.
fn average(days: &[&DayTotal], window: usize) -> Average {
    let days = &days[..days.len().min(window)];
    let count = days.len() as u64;
    let sum: u64 = days.iter().fold(0, |sum, day| sum.saturating_add(day.tokens));
    Average { tokens: sum.saturating_add(count / 2).checked_div(count).unwrap_or(0), days: count }
}

/// O mês de um dia: `AAAA-MM`.
fn month_of(day: &str) -> &str {
    day.get(..7).unwrap_or(day)
}

/// Os dias que faltam no mês de `today`, ele inclusive.
fn days_left_in_month(today: &str) -> u64 {
    let Ok(date) = NaiveDate::parse_from_str(today, DAY_FORMAT) else { return 0 };
    let (year, month) = if date.month() == 12 { (date.year() + 1, 1) } else { (date.year(), date.month() + 1) };
    NaiveDate::from_ymd_opt(year, month, 1).map_or(0, |next| u64::try_from((next - date).num_days()).unwrap_or(0))
}

/// A soma de `field` nos `days`.
fn sum(days: &[&DayTotal], field: fn(&DayTotal) -> u64) -> u64 {
    days.iter().fold(0, |sum, day| sum.saturating_add(field(day)))
}

/// O resumo da máquina inteira quando hoje é `today`. `closed` são as linhas
/// dos dias fechados; `open` as de hoje, que só entram em "hoje até agora". Só
/// os dias com [`MIN_ACTIONS`] ações ou mais entram nas médias; a previsão usa
/// a média dos últimos sete dias fechados com busca.
#[must_use]
pub fn summarize(closed: &[DayRow], open: &[DayRow], today: &str) -> Summary {
    let closed_days = totals(closed.iter().filter(|row| row.day.as_str() < today));
    let today_total = totals(open.iter().filter(|row| row.day == today)).into_iter().next();
    let yesterday = previous_day(today).unwrap_or_default();
    let yesterday_total = closed_days.iter().find(|day| day.day == yesterday).cloned();

    let busy: Vec<&DayTotal> = closed_days.iter().rev().filter(|day| day.actions >= MIN_ACTIONS).collect();
    let this_month: Vec<&DayTotal> = busy.iter().copied().filter(|day| month_of(&day.day) == month_of(today)).collect();
    let month_days: Vec<&DayTotal> = closed_days.iter().filter(|day| month_of(&day.day) == month_of(today)).collect();
    let searched: Vec<&DayTotal> = closed_days.iter().rev().filter(|day| day.jev_tokens > 0).take(LONG_WINDOW).collect();

    let count = searched.len() as u64;
    let days_left = days_left_in_month(today);
    let (spent_micro_usd, spent_tokens) = (sum(&month_days, |d| d.jev_cost_micro_usd), sum(&month_days, |d| d.jev_tokens));
    let (window_cost, window_tokens) = (sum(&searched, |d| d.jev_cost_micro_usd), sum(&searched, |d| d.jev_tokens));
    let ahead = |window: u64| window.saturating_mul(days_left).checked_div(count).unwrap_or(0);
    Summary {
        today: today_total.unwrap_or(DayTotal { day: today.to_string(), ..DayTotal::default() }),
        yesterday: yesterday_total.unwrap_or(DayTotal { day: yesterday, ..DayTotal::default() }),
        last_3: average(&busy, SHORT_WINDOW),
        last_7: average(&busy, LONG_WINDOW),
        month: average(&this_month, usize::MAX),
        forecast: Forecast {
            spent_micro_usd,
            spent_tokens,
            daily_micro_usd: window_cost.checked_div(count).unwrap_or(0),
            daily_tokens: window_tokens.checked_div(count).unwrap_or(0),
            days_left,
            micro_usd: spent_micro_usd.saturating_add(ahead(window_cost)),
            tokens: spent_tokens.saturating_add(ahead(window_tokens)),
        },
        min_actions: MIN_ACTIONS,
    }
}

/// O indicador da barra de status: o último dia de trabalho fechado contra a
/// média dos sete dias de trabalho anteriores a ele.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trend {
    pub day: String,
    pub tokens: u64,
    pub average: Average,
    /// A variação contra a média, em percentual inteiro e com sinal.
    pub change_percent: i64,
}

/// O indicador a partir das linhas dos dias fechados, com o corte das médias da
/// página: um fim de semana parado não vira o dia comparado. `None` sem dia de
/// trabalho fechado, ou quando nenhum veio antes dele.
#[must_use]
pub fn trend(closed: &[DayRow]) -> Option<Trend> {
    let days = totals(closed);
    let busy: Vec<&DayTotal> = days.iter().rev().filter(|day| day.actions >= MIN_ACTIONS).collect();
    let (last, before) = busy.split_first()?;
    let average = average(before, LONG_WINDOW);
    if average.tokens == 0 {
        return None;
    }
    let base = i128::from(average.tokens);
    let delta = i128::from(last.tokens) - base;
    // Meio ponto arredonda para longe do zero: a variação de -12,5% é -13%.
    let change_percent = i64::try_from((delta * 100 + delta.signum() * base / 2) / base).ok()?;
    Some(Trend { day: last.day.clone(), tokens: last.tokens, average, change_percent })
}

/// A faixa de dias que falta contar: do dia seguinte ao último contado (ou do
/// começo das conversas, quando nada foi contado ainda) até ontem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    pub first: Option<String>,
    pub last: String,
}

impl Range {
    /// Se `day` cai dentro da faixa.
    #[must_use]
    pub fn contains(&self, day: &str) -> bool {
        day <= self.last.as_str() && self.first.as_deref().is_none_or(|first| day >= first)
    }
}

/// O arquivo do gasto na máquina: as linhas dos dias já fechados, até onde a
/// conta foi, e o que a página já recebeu. Recontar é apagá-lo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    /// O último dia fechado contado, o endereço da página publicada (um só por
    /// máquina) e o último dia fechado que a cópia mais recente levou a ela.
    #[serde(default)]
    pub counted_through: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub copied_through: Option<String>,
    /// A versão que a página tem dos documentos que a próxima cópia troca, pelo
    /// nome `coleção/doc_id`: as linhas do dia aberto e o resumo. O banco só
    /// troca um documento que já existe quando a escrita traz a versão dele.
    #[serde(default)]
    pub versions: BTreeMap<String, u64>,
    /// As linhas dos dias fechados contados, em ordem de dia e de projeto.
    #[serde(default)]
    pub rows: Vec<DayRow>,
}

impl Ledger {
    /// A faixa que falta contar quando hoje é `today`, até ontem; `None` quando
    /// já foi contada ou `today` não é um dia.
    #[must_use]
    pub fn to_count(&self, today: &str) -> Option<Range> {
        let last = previous_day(today)?;
        let first = match self.counted_through.as_deref() {
            Some(done) if done >= last.as_str() => return None,
            Some(done) => Some(next_day(done)?),
            None => None,
        };
        Some(Range { first, last })
    }

    /// Guarda as linhas de `counted` no lugar das dos dias de `range` e leva a
    /// conta até o fim da faixa.
    pub fn record_counted(&mut self, range: &Range, counted: Vec<DayRow>) {
        self.rows.retain(|row| !range.contains(&row.day));
        self.rows.extend(counted.into_iter().filter(|row| range.contains(&row.day)));
        self.rows.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.project.cmp(&b.project)));
        if self.counted_through.as_deref().is_none_or(|done| done < range.last.as_str()) {
            self.counted_through = Some(range.last.clone());
        }
    }

    /// As linhas que a página ainda não recebeu.
    #[must_use]
    pub fn uncopied(&self) -> Vec<&DayRow> {
        let copied = self.copied_through.as_deref();
        self.rows.iter().filter(|row| copied.is_none_or(|copied| row.day.as_str() > copied)).collect()
    }

    /// Dá por feita a cópia que acabou de ser preparada: a página recebeu as
    /// linhas fechadas até `through`, e cada documento de `docs` (o dia aberto
    /// e o resumo, que a cópia seguinte troca) sobe uma versão. Numa página
    /// nova (`fresh`) o banco está vazio e as versões recomeçam em um.
    pub fn record_sent(&mut self, through: Option<String>, docs: Vec<String>, fresh: bool) {
        if fresh {
            self.copied_through = None;
            self.versions.clear();
        }
        if through.is_some() {
            self.copied_through = through;
        }
        let versions = &self.versions;
        self.versions = docs.into_iter().map(|doc| (doc.clone(), versions.get(&doc).map_or(1, |v| v.saturating_add(1)))).collect();
    }
}

/// Por que o comando do gasto recusa: o endereço não é `https://…`, a pasta da
/// máquina não se acha, o arquivo do gasto não se lê ou o disco falhou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotAnAddress { found: String },
    NoMachineFolder,
    UnreadableLedger { path: String, detail: String },
    Io { detail: String },
}

impl Refusal {
    /// A razão curta e estável, em kebab-case.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::NotAnAddress { .. } => "not-an-address",
            Self::NoMachineFolder => "no-machine-folder",
            Self::UnreadableLedger { .. } => "unreadable-ledger",
            Self::Io { .. } => "io",
        }
    }

    /// A mensagem exata, no idioma pedido.
    #[must_use]
    pub fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, &str)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::NotAnAddress { found } => fill("page.spend.refusal.not_an_address", &[("{found}", found)]),
            Self::NoMachineFolder => fill("page.spend.refusal.no_machine_folder", &[]),
            Self::UnreadableLedger { path, detail } => {
                fill("page.spend.refusal.unreadable_ledger", &[("{path}", path), ("{detail}", detail)])
            }
            Self::Io { detail } => fill("page.spend.refusal.io", &[("{detail}", detail)]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uma linha do dia com `actions` ações e `tokens` tokens, num dia e
    /// projeto.
    fn busy(day: &str, project: &str, actions: u64, tokens: u64) -> DayRow {
        DayRow { day: day.to_string(), project: project.to_string(), tokens, actions, ..DayRow::default() }
    }

    /// O dia é o do fuso de -03:00, qualquer que seja o deslocamento do
    /// carimbo: 02:30 UTC de 2 de outubro ainda é 1º de outubro às 23:30.
    #[test]
    fn the_day_is_the_one_of_the_minus_three_zone() {
        assert_eq!(day_of_stamp("2026-10-02T02:30:00Z").as_deref(), Some("2026-10-01"));
        assert_eq!(day_of_stamp("2026-10-02T03:00:00Z").as_deref(), Some("2026-10-02"));
        assert_eq!(day_of_stamp("2026-10-01T23:30:00-03:00").as_deref(), Some("2026-10-01"));
        assert_eq!(day_of_stamp("não é carimbo"), None);
        assert_eq!(day_start("2026-10-02").map(|at| at.to_rfc3339()).as_deref(), Some("2026-10-02T03:00:00+00:00"));
        assert_eq!(next_day("2026-02-28").as_deref(), Some("2026-03-01"));
        assert_eq!(previous_day("2026-03-01").as_deref(), Some("2026-02-28"));
    }

    /// `Grep`, `Glob`, o agente `Explore` e o `Bash` que começa com `rg`,
    /// `grep` ou `find` são procuras de código; `Read`, o `Bash` com
    /// `mustard-rt` e os outros usos não são.
    #[test]
    fn only_the_search_tools_count_as_code_searches() {
        let search = |name, command: Option<&str>, subagent: Option<&str>| is_code_search(name, command, subagent);
        assert!(search("Grep", None, None) && search("Glob", None, None));
        assert!(search("Agent", None, Some("Explore")) && search("Task", None, Some("Explore")));
        assert!(!search("Agent", None, Some("general-purpose")) && !search("Agent", None, None));
        assert!(search("Bash", Some("  rg --files"), None) && search("Bash", Some("/usr/bin/grep foo"), None));
        assert!(!search("Bash", Some("mustard-rt run map search \"foo\""), None) && !search("Bash", Some("cargo test grep"), None));
        assert!(!search("Read", None, None) && !search("Edit", None, None));
    }

    /// O nome do documento junta o dia e o projeto sem espaço nem símbolo.
    #[test]
    fn the_document_name_joins_the_day_and_the_project() {
        assert_eq!(busy("2026-10-01", "mustard", 1, 1).doc_id(), "2026-10-01-mustard");
        assert_eq!(busy("2026-10-01", "Portal Florestal_API", 1, 1).doc_id(), "2026-10-01-portal-florestal-api");
        assert_eq!(busy("2026-10-01", "???", 1, 1).doc_id(), "2026-10-01-project");
    }

    /// As médias de três e de sete dias levam os últimos dias fechados com
    /// pelo menos cem ações da máquina inteira e pulam o que teve cinquenta,
    /// mesmo quando ele é o mais novo; os projetos de um dia se somam. A média
    /// do mês só usa os dias do mês de hoje, e hoje só entra em "hoje até
    /// agora", com as linhas do dia aberto somadas.
    #[test]
    fn the_averages_use_busy_closed_days_and_today_only_shows_so_far() {
        let mut closed: Vec<DayRow> = (25..=28).map(|day| busy(&format!("2026-09-{day}"), "a", 150, 1000 * (day - 24))).collect();
        closed.extend([
            busy("2026-09-29", "a", 60, 5000),
            busy("2026-09-29", "b", 90, 5000),
            busy("2026-09-30", "a", 150, 6000),
            busy("2026-10-01", "a", 50, 9000),
            busy("2026-10-02", "a", 999, 999_999),
        ]);
        let open = [
            DayRow { partial: true, ..busy("2026-10-02", "a", 300, 7000) },
            DayRow { partial: true, ..busy("2026-10-02", "b", 100, 500) },
        ];
        let summary = summarize(&closed, &open, "2026-10-02");
        assert_eq!(summary.last_3, Average { tokens: 6667, days: 3 }, "{summary:?}");
        assert_eq!(summary.last_7, Average { tokens: 4333, days: 6 });
        assert_eq!(summary.yesterday, DayTotal { day: "2026-10-01".into(), tokens: 9000, actions: 50, ..DayTotal::default() });
        assert_eq!((summary.today.day.as_str(), summary.today.tokens, summary.today.actions), ("2026-10-02", 7500, 400));
        assert_eq!(summary.month, Average::default(), "the only closed day of the month had fifty actions");

        let closed = [busy("2026-09-30", "a", 500, 90_000), busy("2026-10-01", "a", 200, 3000), busy("2026-10-02", "a", 40, 99_999), busy("2026-10-03", "a", 200, 5000)];
        let summary = summarize(&closed, &[], "2026-10-04");
        assert_eq!(summary.month, Average { tokens: 4000, days: 2 }, "September and the 40-action day stay out of it");
    }

    /// Dez dias fechados a US$ 0,02 cada, num mês de 31 dias com hoje no dia
    /// 11, dão US$ 0,62 de previsão: US$ 0,20 já gastos mais US$ 0,02 pelos
    /// 21 dias que faltam, hoje incluído. Em tokens, a mesma conta. A média
    /// leva os últimos sete dias com busca, de qualquer mês, e pula os sem
    /// busca; sem nenhum, a previsão é zero.
    #[test]
    fn the_jev_forecast_of_ten_closed_days_at_two_cents_in_a_31_day_month_is_sixty_two_cents() {
        let searched = |day: String, cost: u64| DayRow { jev_tokens: 700, jev_cost_micro_usd: cost, ..busy(&day, "a", 10, 1) };
        let closed: Vec<DayRow> = (1..=10).map(|day| searched(format!("2026-10-{day:02}"), 20_000)).collect();
        let forecast = summarize(&closed, &[], "2026-10-11").forecast;
        assert_eq!((forecast.days_left, forecast.spent_micro_usd, forecast.daily_micro_usd), (21, 200_000, 20_000));
        assert_eq!(forecast.micro_usd, 620_000, "US$ 0,62");
        assert_eq!((forecast.spent_tokens, forecast.daily_tokens, forecast.tokens), (7000, 700, 7000 + 700 * 21));

        let mut closed: Vec<DayRow> = (20_u64..=29).map(|day| searched(format!("2026-09-{day}"), 1000 * day)).collect();
        closed.push(busy("2026-09-30", "a", 500, 1));
        let forecast = summarize(&closed, &[], "2026-10-01").forecast;
        assert_eq!(forecast.daily_micro_usd, (23 + 24 + 25 + 26 + 27 + 28 + 29) * 1000 / 7, "the last seven with a search");
        let quiet = summarize(&[busy("2026-09-30", "a", 500, 1)], &[], "2026-10-05").forecast;
        assert_eq!((quiet.micro_usd, quiet.daily_micro_usd), (0, 0));
        assert_eq!((days_left_in_month("2026-12-31"), days_left_in_month("2026-02-01")), (1, 28));
    }

    /// Sete dias fechados a 120 milhões de tokens e um último dia a 90
    /// milhões dão menos 25%: o indicador compara o último dia fechado com a
    /// média dos dias de antes. Dia com menos de cem ações fica fora da média
    /// e nunca vira o dia comparado, só os sete dias anteriores entram, meio
    /// ponto arredonda para longe do zero, e sem dia de trabalho antes não há
    /// média contra a qual comparar.
    #[test]
    fn a_last_day_of_ninety_million_against_an_average_of_one_hundred_twenty_million_is_minus_twenty_five() {
        let mut closed: Vec<DayRow> = (13..=19).map(|day| busy(&format!("2026-09-{day}"), "a", 300, 120_000_000)).collect();
        closed.insert(0, busy("2026-09-12", "a", 300, 8_000_000_000));
        closed.extend([busy("2026-09-20", "a", 300, 50_000_000), busy("2026-09-20", "b", 300, 40_000_000)]);
        closed.extend([busy("2026-09-21", "a", 5, 1_000), busy("2026-09-22", "a", 5, 2_000)]);
        let found = trend(&closed).unwrap();
        assert_eq!((found.day.as_str(), found.tokens), ("2026-09-20", 90_000_000), "the projects add up: {found:?}");
        assert_eq!(found.average, Average { tokens: 120_000_000, days: 7 }, "the eighth day back is left out");
        assert_eq!(found.change_percent, -25);

        let around = |last: u64| trend(&[busy("2026-09-26", "a", 300, 200), busy("2026-09-27", "a", 100, last)]).unwrap().change_percent;
        assert_eq!((around(175), around(225), around(201), around(199), around(212)), (-13, 13, 1, -1, 6));
        assert_eq!(trend(&[]), None, "no closed day");
        assert_eq!(trend(&[busy("2026-09-27", "a", 300, 5000)]), None, "no day before the last");
        let idle = [busy("2026-09-25", "a", 99, 5000), busy("2026-09-26", "a", 10, 5000), busy("2026-09-27", "a", 300, 5000)];
        assert_eq!(trend(&idle), None, "the days before had no work");
    }

    /// O que falta contar é do dia seguinte ao último contado até ontem; o dia
    /// de hoje nunca entra, e o que já foi contado não volta. Gravar uma
    /// contagem troca as linhas dos dias da faixa, deixa as de antes e leva a
    /// conta até o fim da faixa.
    #[test]
    fn the_days_to_count_run_to_yesterday_and_a_count_replaces_only_the_days_of_its_range() {
        let mut ledger = Ledger::default();
        assert_eq!(ledger.to_count("2026-10-02"), Some(Range { first: None, last: "2026-10-01".into() }));
        ledger.counted_through = Some("2026-09-29".into());
        ledger.rows = vec![busy("2026-09-29", "a", 1, 1)];
        let range = ledger.to_count("2026-10-02").unwrap();
        assert_eq!(range, Range { first: Some("2026-09-30".into()), last: "2026-10-01".into() });
        ledger.record_counted(&range, vec![busy("2026-10-01", "b", 1, 1), busy("2026-09-30", "a", 1, 1), busy("2026-10-05", "z", 1, 1)]);
        let seen: Vec<(&str, &str)> = ledger.rows.iter().map(|r| (r.day.as_str(), r.project.as_str())).collect();
        assert_eq!(seen, [("2026-09-29", "a"), ("2026-09-30", "a"), ("2026-10-01", "b")]);
        assert_eq!(ledger.to_count("2026-10-02"), None, "a closed day is never counted twice");
        assert_eq!(ledger.to_count("sem dia"), None);
    }

    /// A cópia leva cada dia fechado uma vez e sobe a versão de cada documento
    /// que a cópia seguinte troca: só o resumo e as linhas do dia aberto
    /// ficam guardados, e os de um dia que fechou saem. Uma página nova
    /// recomeça: todas as linhas voltam e as versões partem de um.
    #[test]
    fn a_copy_sends_each_closed_day_once_and_a_new_page_starts_over() {
        let mut ledger = Ledger { rows: vec![busy("2026-09-30", "a", 1, 1), busy("2026-10-01", "a", 1, 1)], ..Ledger::default() };
        let docs = || vec!["days/2026-10-02-a".to_string(), "summary/current".to_string()];
        assert_eq!(ledger.uncopied().len(), 2, "nothing was copied yet");
        ledger.record_sent(Some("2026-10-01".into()), docs(), false);
        assert!(ledger.uncopied().is_empty(), "the closed lines went");
        ledger.record_sent(None, docs(), false);
        assert_eq!(ledger.versions.get("summary/current"), Some(&2), "a replaced document rises one version");
        assert_eq!(ledger.copied_through.as_deref(), Some("2026-10-01"), "a copy with no closed line keeps the day");
        ledger.record_sent(None, vec!["summary/current".into()], false);
        assert_eq!(ledger.versions.keys().collect::<Vec<_>>(), ["summary/current"], "the day that closed is not pinned anymore");

        ledger.record_sent(None, docs(), true);
        assert_eq!(ledger.versions.get("summary/current"), Some(&1), "the new database starts at version 1");
        assert_eq!(ledger.uncopied().len(), 2, "a new page gets every closed line again");
    }
}
