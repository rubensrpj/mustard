//! O gasto de cada dia: a conta pura, sem disco e sem relógio.
//!
//! O Claude Code grava cada conversa na máquina, com os tokens de cada resposta
//! e cada ferramenta usada. O Mustard conta cada dia fechado por essas
//! gravações, um projeto por vez, e guarda a linha de cada dia num arquivo da
//! máquina (o [`Ledger`]); a página do gasto lê essas linhas de um banco de
//! dados, como a página da spec.
//!
//! Aqui moram as regras que não tocam o disco: o dia no fuso de -03:00, o que
//! cada ferramenta usada conta ([`classify_tool`]), a linha de um dia por
//! projeto ([`DayRow`]), o arquivo da máquina ([`Ledger`]), que diz que dias
//! faltam contar e que linhas faltam copiar, e o resumo do topo da página
//! ([`summarize`]): hoje até agora, ontem, as médias e a previsão do Jev. O dia
//! de hoje é um dia aberto: ele entra no resumo e na página como parcial, e
//! nunca no arquivo dos dias fechados. A leitura das conversas e do arquivo
//! mora em `io::spend` e `io::transcript`.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Offset, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::platform::i18n::{translate, Locale};

/// Quantas horas a oeste de UTC o dia é contado: -03:00.
const ZONE_HOURS_WEST: i32 = 3;

/// O formato de um dia: `2026-10-01`.
const DAY_FORMAT: &str = "%Y-%m-%d";

/// O fuso em que o dia é contado, o mesmo em toda máquina: o dia de uma
/// conversa não muda conforme o relógio de quem roda a conta.
#[must_use]
pub fn zone() -> FixedOffset {
    FixedOffset::west_opt(ZONE_HOURS_WEST * 3600).unwrap_or_else(|| Utc.fix())
}

/// O dia em que o instante `at` cai, no fuso do gasto.
#[must_use]
pub fn day_of(at: DateTime<Utc>) -> String {
    at.with_timezone(&zone()).format(DAY_FORMAT).to_string()
}

/// O dia de um carimbo em RFC 3339, com o deslocamento que ele trouxer: a
/// conversa grava em UTC e a spec grava em hora local, e os dois caem no mesmo
/// dia do fuso do gasto. `None` quando o texto não é um carimbo.
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

/// Se `text` é um dia: `AAAA-MM-DD`, de um dia que existe.
#[must_use]
pub fn is_day(text: &str) -> bool {
    NaiveDate::parse_from_str(text, DAY_FORMAT).is_ok_and(|date| date.format(DAY_FORMAT).to_string() == text)
}

/// O instante em que `day` começa no fuso do gasto, em UTC. Um arquivo de
/// conversa que não mudou desde esse instante não tem linha do dia nem de
/// depois dele.
#[must_use]
pub fn day_start(day: &str) -> Option<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(day, DAY_FORMAT).ok()?;
    let midnight = date.and_hms_opt(0, 0, 0)?;
    zone().from_local_datetime(&midnight).single().map(|at| at.with_timezone(&Utc))
}

/// O que um uso de ferramenta soma, além de ser uma ação: toda ferramenta
/// usada é uma ação; estas são as colunas que ela pode somar a mais.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tool {
    /// É uma procura de código: `Grep`, `Glob`, o agente `Explore` e o `Bash`
    /// cujo comando começa com `rg`, `grep` ou `find`.
    pub code_search: bool,
    /// É a leitura de um arquivo (`Read`). Ler um arquivo para editar não é
    /// procurar, e por isso a leitura nunca é uma procura.
    pub file_read: bool,
    /// É uma busca do Mustard feita por comando: o `Bash` com
    /// `mustard-rt run map search`.
    pub mustard_search: bool,
}

/// O agente que procura código por conta própria.
const EXPLORE: &str = "Explore";

/// O que o uso da ferramenta `name` soma. `command` é o comando de um `Bash` e
/// `subagent` o tipo de agente de um `Agent` (`Task`, nas conversas mais
/// antigas).
///
/// O `Bash` conta como procura só quando o comando começa com `rg`, `grep` ou
/// `find`; o que começa com `mustard-rt` não é procura de código, e a busca do
/// Mustard por comando tem a coluna própria.
#[must_use]
pub fn classify_tool(name: &str, command: Option<&str>, subagent: Option<&str>) -> Tool {
    match name {
        "Grep" | "Glob" => Tool { code_search: true, ..Tool::default() },
        "Read" => Tool { file_read: true, ..Tool::default() },
        "Agent" | "Task" => Tool { code_search: subagent == Some(EXPLORE), ..Tool::default() },
        "Bash" => {
            let command = command.unwrap_or_default();
            Tool {
                code_search: starts_with_search_program(command),
                mustard_search: runs_mustard_search(command),
                ..Tool::default()
            }
        }
        _ => Tool::default(),
    }
}

/// Se o comando começa por um dos programas de procura. O caminho do programa
/// não conta: `/usr/bin/grep` é `grep`.
fn starts_with_search_program(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or_default();
    let program = first.rsplit(['/', '\\']).next().unwrap_or(first);
    matches!(program, "rg" | "grep" | "find")
}

/// Se o comando roda a busca do Mustard por comando, em qualquer lugar da
/// linha, com os espaços de sobra e o `.exe` do Windows ignorados.
fn runs_mustard_search(command: &str) -> bool {
    let flat = command.split_whitespace().collect::<Vec<_>>().join(" ").replace("mustard-rt.exe", "mustard-rt");
    flat.contains("mustard-rt run map search")
}

/// A folga, em segundos, entre o uso do `Bash` que roda a busca do Mustard e a
/// chamada `word search` que a busca grava na spec: o uso é carimbado quando a
/// resposta do modelo termina, a chamada quando a busca acaba, e o carimbo da
/// spec não tem fração de segundo.
const PAIR_SLACK_SECONDS: i64 = 30;

/// O teto, em milissegundos, do tempo da busca que a janela de pareamento
/// aceita: uma duração gravada absurda não abre a janela para o dia inteiro.
const PAIR_MAX_MS: u64 = 600_000;

/// Quais dos usos de `Bash` com `mustard-rt run map search` (`commands`, o
/// carimbo de cada um) já estão nas chamadas `word search` gravadas na spec
/// (`events`, o carimbo e a duração em milissegundos de cada uma): a mesma
/// busca é uma só, e não duas. Cada chamada leva o uso mais próximo, antes
/// dela, que começou até a duração da busca mais uma folga antes de ela ser
/// gravada; cada uso só pareia com uma chamada. Devolve a posição, em
/// `commands`, de cada uso pareado.
#[must_use]
pub fn paired_searches(commands: &[DateTime<Utc>], events: &[(DateTime<Utc>, u64)]) -> Vec<usize> {
    let mut events: Vec<&(DateTime<Utc>, u64)> = events.iter().collect();
    events.sort_by_key(|(at, _)| *at);
    let mut used = vec![false; commands.len()];
    for (at, ms) in events {
        let window = Duration::milliseconds(i64::try_from((*ms).min(PAIR_MAX_MS)).unwrap_or(0))
            + Duration::seconds(PAIR_SLACK_SECONDS);
        let nearest = commands
            .iter()
            .enumerate()
            .filter(|(n, started)| !used[*n] && **started <= *at + Duration::seconds(1) && *at - **started <= window)
            .max_by_key(|(_, started)| **started)
            .map(|(n, _)| n);
        if let Some(n) = nearest {
            used[n] = true;
        }
    }
    used.iter().enumerate().filter(|(_, paired)| **paired).map(|(n, _)| n).collect()
}

/// A linha de um dia de um projeto.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayRow {
    /// O dia, `AAAA-MM-DD`, no fuso do gasto.
    pub day: String,
    /// O nome da pasta do projeto, lido do `cwd` gravado na conversa.
    pub project: String,
    /// Entrada, criação de cache, leitura de cache e saída de cada resposta,
    /// cada resposta contada uma vez.
    #[serde(default)]
    pub tokens: u64,
    /// Cada uso de ferramenta.
    #[serde(default)]
    pub actions: u64,
    /// As ações que são procuras de código ([`Tool::code_search`]).
    #[serde(default)]
    pub code_searches: u64,
    /// As leituras de arquivo ([`Tool::file_read`]), em coluna própria.
    #[serde(default)]
    pub file_reads: u64,
    /// As buscas do Mustard: as que o gancho faz sozinho (a chamada
    /// `word search` gravada na spec) e os `Bash` com `mustard-rt run map
    /// search`.
    #[serde(default)]
    pub mustard_searches: u64,
    /// Das chamadas `word search`, as que o Jev respondeu vazias
    /// (`returned` zero).
    #[serde(default)]
    pub empty_searches: u64,
    /// Os tokens que o Jev gastou nas chamadas `word search`.
    #[serde(default)]
    pub jev_tokens: u64,
    /// O custo do Jev nas chamadas `word search`, em milionésimos de dólar.
    #[serde(default)]
    pub jev_cost_micro_usd: u64,
    /// A linha é de um dia aberto (hoje): a conta dele ainda cresce, e a
    /// página a marca como parcial. Uma linha do arquivo dos dias fechados
    /// nunca é parcial.
    #[serde(default, skip_serializing_if = "is_false")]
    pub partial: bool,
}

/// Se o valor é falso: o campo `partial` só é gravado quando é verdadeiro.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

impl DayRow {
    /// O nome do documento da linha no banco da página: o dia e o projeto em
    /// letras minúsculas, números e hífens, único por dia e projeto.
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
        let slug = if slug.is_empty() { "project" } else { slug };
        format!("{}-{slug}", self.day)
    }

    /// O corpo do documento que vai para o banco da página.
    #[must_use]
    pub fn body(&self) -> Value {
        json!({
            "day": self.day, "project": self.project, "tokens": self.tokens, "actions": self.actions,
            "code_searches": self.code_searches, "file_reads": self.file_reads,
            "mustard_searches": self.mustard_searches, "empty_searches": self.empty_searches,
            "jev_tokens": self.jev_tokens, "jev_cost_micro_usd": self.jev_cost_micro_usd,
            "partial": self.partial,
        })
    }
}

/// O mínimo de ações de um dia, da máquina inteira, para ele entrar numa
/// média: um dia quase parado não puxa a média para baixo.
pub const MIN_ACTIONS: u64 = 100;

/// Quantos dias a média curta leva, e a longa.
const SHORT_WINDOW: usize = 3;
const LONG_WINDOW: usize = 7;

/// O consumo de um dia da máquina inteira: os projetos somados.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DayTotal {
    /// O dia, `AAAA-MM-DD`.
    pub day: String,
    /// Os tokens do dia: entrada, criação de cache, leitura de cache e saída.
    pub tokens: u64,
    /// Os usos de ferramenta do dia.
    pub actions: u64,
    /// O dia teve busca do Mustard (a do Jev ou a do mapa).
    pub searched: bool,
    /// Os tokens que o Jev gastou no dia.
    pub jev_tokens: u64,
    /// O custo do Jev no dia, em milionésimos de dólar.
    pub jev_cost_micro_usd: u64,
}

/// A média de tokens por dia de uma janela e quantos dias entraram nela.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Average {
    /// Tokens por dia, arredondado.
    pub tokens: u64,
    /// Quantos dias entraram na conta; menos que a janela quando faltam
    /// dias com ações bastantes.
    pub days: u64,
}

/// A previsão do gasto do Jev no mês do calendário.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Forecast {
    /// Gasto dos dias fechados do mês, em milionésimos de dólar.
    pub spent_micro_usd: u64,
    /// Tokens do Jev nos dias fechados do mês.
    pub spent_tokens: u64,
    /// A média por dia dos últimos dias com busca, em milionésimos de dólar.
    pub daily_micro_usd: u64,
    /// A média por dia dos últimos dias com busca, em tokens.
    pub daily_tokens: u64,
    /// Os dias que faltam no mês, hoje inclusive.
    pub days_left: u64,
    /// O gasto previsto no mês: o dos dias fechados mais a média por dia
    /// vezes os dias que faltam, em milionésimos de dólar.
    pub micro_usd: u64,
    /// O mesmo, em tokens.
    pub tokens: u64,
}

/// O resumo do topo da página: o consumo da máquina inteira.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// Hoje até agora: o dia aberto, que ainda cresce.
    pub today: DayTotal,
    /// Ontem, o último dia fechado.
    pub yesterday: DayTotal,
    /// A média dos últimos três dias fechados com ações bastantes.
    pub last_3: Average,
    /// A média dos últimos sete dias fechados com ações bastantes.
    pub last_7: Average,
    /// A média dos dias fechados do mês do calendário com ações bastantes.
    pub month: Average,
    /// A previsão do Jev no mês.
    pub forecast: Forecast,
    /// O mínimo de ações de um dia para entrar nas médias, que a página diz.
    pub min_actions: u64,
}

/// A coleção do banco da página onde o resumo mora, e o nome do documento.
pub const SUMMARY_COLLECTION: &str = "summary";
pub const SUMMARY_DOC: &str = "current";

impl Summary {
    /// O corpo do documento do resumo no banco da página.
    #[must_use]
    pub fn body(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// Os projetos somados, um total por dia, do dia mais velho ao mais novo.
fn totals<'a>(rows: impl IntoIterator<Item = &'a DayRow>) -> Vec<DayTotal> {
    let mut days: BTreeMap<&str, DayTotal> = BTreeMap::new();
    for row in rows {
        let total = days.entry(row.day.as_str()).or_insert_with(|| DayTotal { day: row.day.clone(), ..DayTotal::default() });
        total.tokens = total.tokens.saturating_add(row.tokens);
        total.actions = total.actions.saturating_add(row.actions);
        total.jev_tokens = total.jev_tokens.saturating_add(row.jev_tokens);
        total.jev_cost_micro_usd = total.jev_cost_micro_usd.saturating_add(row.jev_cost_micro_usd);
        total.searched |= row.mustard_searches > 0 || row.jev_tokens > 0 || row.jev_cost_micro_usd > 0;
    }
    days.into_values().collect()
}

/// A média de tokens por dia de `days`, arredondada.
fn average(days: &[&DayTotal]) -> Average {
    let count = days.len() as u64;
    let sum: u64 = days.iter().fold(0, |sum, day| sum.saturating_add(day.tokens));
    Average { tokens: sum.saturating_add(count / 2).checked_div(count).unwrap_or(0), days: count }
}

/// O mês de um dia: `AAAA-MM`.
fn month_of(day: &str) -> &str {
    day.get(..7).unwrap_or(day)
}

/// Os dias que faltam no mês de `today`, ele inclusive: num mês de 31 dias, no
/// dia 11 faltam 21.
fn days_left_in_month(today: &str) -> u64 {
    let Ok(date) = NaiveDate::parse_from_str(today, DAY_FORMAT) else { return 0 };
    let next = if date.month() == 12 {
        NaiveDate::from_ymd_opt(date.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() + 1, 1)
    };
    next.map_or(0, |next| u64::try_from((next - date).num_days()).unwrap_or(0))
}

/// O resumo da máquina inteira quando hoje é `today`. `closed` são as linhas
/// dos dias fechados; `open` as de hoje, que ainda crescem e só entram em "hoje
/// até agora". Só os dias com [`MIN_ACTIONS`] ações ou mais entram nas médias.
///
/// A previsão do Jev soma o custo dos dias fechados do mês e a média por dia
/// dos últimos sete dias fechados com busca vezes os dias que faltam no mês.
#[must_use]
pub fn summarize(closed: &[DayRow], open: &[DayRow], today: &str) -> Summary {
    let closed_days = totals(closed.iter().filter(|row| row.day.as_str() < today));
    let today_total = totals(open.iter().filter(|row| row.day == today)).into_iter().next();
    let yesterday = previous_day(today).unwrap_or_default();
    let yesterday_total = closed_days.iter().find(|day| day.day == yesterday).cloned();

    let busy: Vec<&DayTotal> = closed_days.iter().rev().filter(|day| day.actions >= MIN_ACTIONS).collect();
    let this_month: Vec<&DayTotal> = busy.iter().copied().filter(|day| month_of(&day.day) == month_of(today)).collect();

    let month_days: Vec<&DayTotal> = closed_days.iter().filter(|day| month_of(&day.day) == month_of(today)).collect();
    let searched: Vec<&DayTotal> = closed_days.iter().rev().filter(|day| day.searched).take(LONG_WINDOW).collect();
    let count = searched.len() as u64;
    let window_cost: u64 = searched.iter().fold(0, |sum, day| sum.saturating_add(day.jev_cost_micro_usd));
    let window_tokens: u64 = searched.iter().fold(0, |sum, day| sum.saturating_add(day.jev_tokens));
    let days_left = days_left_in_month(today);
    let spent_micro_usd = month_days.iter().fold(0u64, |sum, day| sum.saturating_add(day.jev_cost_micro_usd));
    let spent_tokens = month_days.iter().fold(0u64, |sum, day| sum.saturating_add(day.jev_tokens));
    let ahead = |window: u64| window.saturating_mul(days_left).checked_div(count).unwrap_or(0);
    Summary {
        today: today_total.unwrap_or(DayTotal { day: today.to_string(), ..DayTotal::default() }),
        yesterday: yesterday_total.unwrap_or(DayTotal { day: yesterday, ..DayTotal::default() }),
        last_3: average(&busy.iter().copied().take(SHORT_WINDOW).collect::<Vec<_>>()),
        last_7: average(&busy.iter().copied().take(LONG_WINDOW).collect::<Vec<_>>()),
        month: average(&this_month),
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

/// O indicador do consumo da barra de status: o último dia de trabalho
/// fechado contra a média dos sete dias de trabalho fechados anteriores a ele.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trend {
    /// O último dia fechado com [`MIN_ACTIONS`] ações ou mais, `AAAA-MM-DD`.
    pub day: String,
    /// Os tokens desse dia, a máquina inteira.
    pub tokens: u64,
    /// A média dos dias anteriores com ações bastantes.
    pub average: Average,
    /// A variação dele contra a média, em percentual inteiro e com sinal:
    /// abaixo da média é negativo.
    pub change_percent: i64,
}

/// O indicador do consumo a partir das linhas dos dias fechados: o dia fechado
/// mais novo com [`MIN_ACTIONS`] ações ou mais, a máquina inteira, contra a
/// média dos sete dias fechados anteriores a ele com o mesmo mínimo — o mesmo
/// corte das médias da página, uma regra só: um fim de semana parado depois da
/// sexta não vira o dia comparado. A variação é arredondada para o percentual
/// inteiro mais próximo.
///
/// `None` sem dia de trabalho fechado, ou quando nenhum dia de trabalho veio
/// antes dele: não há média contra a qual comparar.
#[must_use]
pub fn trend(closed: &[DayRow]) -> Option<Trend> {
    let days = totals(closed);
    let busy: Vec<&DayTotal> = days.iter().rev().filter(|day| day.actions >= MIN_ACTIONS).collect();
    let (last, before) = busy.split_first()?;
    let average = average(&before.iter().copied().take(LONG_WINDOW).collect::<Vec<_>>());
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
    /// O primeiro dia a contar; `None` quando nada foi contado ainda.
    pub first: Option<String>,
    /// O último dia a contar: o dia fechado mais recente.
    pub last: String,
}

impl Range {
    /// Se `day` cai dentro da faixa.
    #[must_use]
    pub fn contains(&self, day: &str) -> bool {
        day <= self.last.as_str() && self.first.as_deref().is_none_or(|first| day >= first)
    }
}

/// O arquivo do gasto na máquina, fora de qualquer projeto: as linhas dos dias
/// já fechados, até onde a conta foi, e o que a página já recebeu. Recontar é
/// apagar o arquivo e deixar o comando refazê-lo pelas conversas.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    /// O último dia fechado que já foi contado; `None` antes da primeira
    /// conta.
    #[serde(default)]
    pub counted_through: Option<String>,
    /// O endereço da página publicada, um só por máquina; `None` enquanto ela
    /// não foi publicada.
    #[serde(default)]
    pub url: Option<String>,
    /// O último dia das linhas que a última preparação mandou copiar.
    #[serde(default)]
    pub prepared_through: Option<String>,
    /// O último dia das linhas que já estão no banco da página.
    #[serde(default)]
    pub copied_through: Option<String>,
    /// Os documentos que a última preparação mandou copiar, pelo nome
    /// `coleção/doc_id`: a versão de cada um sobe quando a cópia é gravada
    /// como feita.
    #[serde(default)]
    pub prepared: Vec<String>,
    /// A versão que cada documento do banco da página tem, pelo nome
    /// `coleção/doc_id`: o banco só troca um documento que já existe quando a
    /// escrita traz a versão dele. A que um documento ganha a cada cópia
    /// feita é a anterior mais um.
    #[serde(default)]
    pub versions: BTreeMap<String, u64>,
    /// As linhas dos dias fechados contados, em ordem de dia e de projeto.
    /// Hoje nunca está aqui.
    #[serde(default)]
    pub rows: Vec<DayRow>,
}

impl Ledger {
    /// A faixa que falta contar quando hoje é `today`: ontem é o dia fechado
    /// mais recente, e a conta vai até ele. `None` quando já foi contado, ou
    /// quando `today` não é um dia.
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

    /// Guarda as linhas de `counted` no lugar das que o arquivo tinha para os
    /// dias de `range`, em ordem, e leva a conta até o último dia da faixa.
    pub fn record_counted(&mut self, range: &Range, counted: Vec<DayRow>) {
        self.rows.retain(|row| !range.contains(&row.day));
        self.rows.extend(counted.into_iter().filter(|row| range.contains(&row.day)));
        self.rows.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.project.cmp(&b.project)));
        if self.counted_through.as_deref().is_none_or(|done| done < range.last.as_str()) {
            self.counted_through = Some(range.last.clone());
        }
    }

    /// As linhas que a página ainda não recebeu: as de depois do último dia
    /// copiado, ou todas, enquanto nada foi copiado.
    #[must_use]
    pub fn uncopied(&self) -> Vec<&DayRow> {
        self.rows
            .iter()
            .filter(|row| self.copied_through.as_deref().is_none_or(|copied| row.day.as_str() > copied))
            .collect()
    }

    /// Grava o que a preparação mandou copiar: até que dia as linhas fechadas
    /// vão (`through`, quando ela levou alguma) e quais documentos ela levou.
    pub fn record_prepared(&mut self, through: Option<String>, docs: Vec<String>) {
        if through.is_some() {
            self.prepared_through = through;
        }
        self.prepared = docs;
    }

    /// Grava a cópia preparada como feita: a página já tem as linhas fechadas
    /// até o último dia que a preparação levou, e cada documento dela subiu
    /// uma versão. `false` quando não há cópia preparada a gravar.
    pub fn record_copy(&mut self) -> bool {
        if self.prepared.is_empty() {
            return false;
        }
        for doc in std::mem::take(&mut self.prepared) {
            let version = self.versions.entry(doc).or_insert(0);
            *version = version.saturating_add(1);
        }
        if self.prepared_through.is_some() {
            self.copied_through.clone_from(&self.prepared_through);
        }
        true
    }

    /// Grava o endereço da página. Outro endereço que o de antes é uma página
    /// nova, com o banco vazio: nada do que foi copiado, nem a versão de
    /// documento nenhum, conta mais; o que a última preparação levou fica
    /// para a cópia que vem em seguida.
    pub fn record_url(&mut self, url: &str) {
        if self.url.as_deref() != Some(url) {
            self.copied_through = None;
            self.versions.clear();
        }
        self.url = Some(url.to_string());
    }
}

/// Por que o comando do gasto recusa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// O endereço dado não é um endereço de página (`https://…`).
    NotAnAddress { found: String },
    /// Não há cópia preparada a gravar como feita.
    NothingPrepared,
    /// A pasta da máquina onde o arquivo do gasto mora não se acha.
    NoMachineFolder,
    /// O arquivo do gasto existe e não se lê.
    UnreadableLedger { path: String, detail: String },
    /// O disco falhou ao ler ou gravar.
    Io { detail: String },
}

impl Refusal {
    /// A razão curta e estável, em kebab-case.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::NotAnAddress { .. } => "not-an-address",
            Self::NothingPrepared => "nothing-prepared",
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
            Self::NothingPrepared => fill("page.spend.refusal.nothing_prepared", &[]),
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

    fn row(day: &str, project: &str) -> DayRow {
        DayRow { day: day.to_string(), project: project.to_string(), actions: 1, ..DayRow::default() }
    }

    /// O dia é o do fuso de -03:00, qualquer que seja o deslocamento do
    /// carimbo: 02:30 UTC de 2 de outubro ainda é 1º de outubro às 23:30, e a
    /// hora local da spec cai no mesmo dia.
    #[test]
    fn the_day_is_the_one_of_the_minus_three_zone() {
        assert_eq!(day_of_stamp("2026-10-02T02:30:00Z").as_deref(), Some("2026-10-01"));
        assert_eq!(day_of_stamp("2026-10-02T03:00:00Z").as_deref(), Some("2026-10-02"));
        assert_eq!(day_of_stamp("2026-10-01T23:30:00-03:00").as_deref(), Some("2026-10-01"));
        assert_eq!(day_of_stamp("2026-10-02T05:30:00+03:00").as_deref(), Some("2026-10-01"));
        assert_eq!(day_of_stamp("não é carimbo"), None);
        assert_eq!(day_start("2026-10-02").map(|at| at.to_rfc3339()).as_deref(), Some("2026-10-02T03:00:00+00:00"));
        assert_eq!(next_day("2026-02-28").as_deref(), Some("2026-03-01"));
        assert_eq!(previous_day("2026-03-01").as_deref(), Some("2026-02-28"));
        assert!(is_day("2026-10-01") && !is_day("2026-13-01") && !is_day("2026-1-1") && !is_day("ontem"));
    }

    /// `Grep`, `Glob`, o agente `Explore` e o `Bash` que começa com `rg`,
    /// `grep` ou `find` são procuras de código; o `Bash` com `mustard-rt` não
    /// é, e a busca do Mustard por comando tem a coluna própria.
    #[test]
    fn only_the_search_tools_count_as_code_searches() {
        let search = |name, command: Option<&str>, subagent: Option<&str>| classify_tool(name, command, subagent).code_search;
        assert!(search("Grep", None, None) && search("Glob", None, None));
        assert!(search("Agent", None, Some("Explore")) && search("Task", None, Some("Explore")));
        assert!(!search("Agent", None, Some("general-purpose")) && !search("Agent", None, None));
        assert!(search("Bash", Some("grep -rn foo src"), None));
        assert!(search("Bash", Some("  rg --files"), None));
        assert!(search("Bash", Some("find . -name '*.rs'"), None));
        assert!(search("Bash", Some("/usr/bin/grep foo"), None));
        assert!(!search("Bash", Some("mustard-rt run map search \"foo\""), None));
        assert!(!search("Bash", Some("cargo test grep"), None));
        assert!(!search("Bash", Some("git log --grep=x"), None));
        assert!(!search("Bash", None, None));
        assert!(!search("Edit", None, None));
    }

    /// `Read` vai para a coluna das leituras e nunca para a das procuras.
    #[test]
    fn a_read_is_a_file_read_and_never_a_search() {
        assert_eq!(classify_tool("Read", None, None), Tool { file_read: true, ..Tool::default() });
        assert!(!classify_tool("Read", None, None).code_search);
        assert!(!classify_tool("Grep", None, None).file_read);
    }

    /// O `Bash` com `mustard-rt run map search`, em qualquer lugar da linha,
    /// é a busca do Mustard por comando; outros comandos do `mustard-rt` não.
    #[test]
    fn the_map_search_command_is_a_mustard_search() {
        let mustard = |command: &str| classify_tool("Bash", Some(command), None).mustard_search;
        assert!(mustard("mustard-rt run map search \"frete\""));
        assert!(mustard("cd /x && mustard-rt  run   map search frete"));
        assert!(mustard("mustard-rt.exe run map search frete"));
        assert!(!mustard("mustard-rt run read spec"));
        assert!(!mustard("grep map search"));
    }

    /// O nome do documento junta o dia e o projeto sem espaço nem símbolo.
    #[test]
    fn the_document_name_joins_the_day_and_the_project() {
        assert_eq!(row("2026-10-01", "mustard").doc_id(), "2026-10-01-mustard");
        assert_eq!(row("2026-10-01", "Portal Florestal_API").doc_id(), "2026-10-01-portal-florestal-api");
        assert_eq!(row("2026-10-01", "???").doc_id(), "2026-10-01-project");
    }

    /// O que falta contar é do dia seguinte ao último contado até ontem; o dia
    /// de hoje nunca entra, e o que já foi contado não volta.
    #[test]
    fn the_days_to_count_run_from_the_day_after_the_last_counted_to_yesterday() {
        let mut ledger = Ledger::default();
        assert_eq!(ledger.to_count("2026-10-02"), Some(Range { first: None, last: "2026-10-01".into() }));
        ledger.counted_through = Some("2026-09-29".into());
        assert_eq!(
            ledger.to_count("2026-10-02"),
            Some(Range { first: Some("2026-09-30".into()), last: "2026-10-01".into() })
        );
        ledger.counted_through = Some("2026-10-01".into());
        assert_eq!(ledger.to_count("2026-10-02"), None, "a closed day is never counted twice");
        assert_eq!(ledger.to_count("sem dia"), None);
    }

    /// Gravar uma contagem troca as linhas dos dias da faixa, deixa as de
    /// antes e leva a conta até o fim da faixa.
    #[test]
    fn recording_a_count_replaces_the_days_of_the_range_only() {
        let mut ledger = Ledger {
            counted_through: Some("2026-09-29".into()),
            rows: vec![row("2026-09-29", "a")],
            ..Ledger::default()
        };
        let range = Range { first: Some("2026-09-30".into()), last: "2026-10-01".into() };
        ledger.record_counted(&range, vec![row("2026-10-01", "b"), row("2026-09-30", "a"), row("2026-10-05", "z")]);
        let seen: Vec<(&str, &str)> = ledger.rows.iter().map(|r| (r.day.as_str(), r.project.as_str())).collect();
        assert_eq!(seen, [("2026-09-29", "a"), ("2026-09-30", "a"), ("2026-10-01", "b")]);
        assert_eq!(ledger.counted_through.as_deref(), Some("2026-10-01"));
    }

    /// As linhas por copiar são as de depois do último dia copiado; a cópia
    /// gravada como feita sobe a versão de cada documento levado, leva o
    /// último dia copiado até onde a preparação foi e não se grava duas vezes.
    #[test]
    fn a_recorded_copy_raises_each_version_once_and_moves_the_copied_day() {
        let mut ledger = Ledger {
            counted_through: Some("2026-10-01".into()),
            rows: vec![row("2026-09-30", "a"), row("2026-10-01", "a")],
            copied_through: Some("2026-09-30".into()),
            ..Ledger::default()
        };
        assert_eq!(ledger.uncopied().len(), 1);
        assert!(!ledger.record_copy(), "nothing was prepared");
        ledger.record_prepared(Some("2026-10-01".into()), vec!["days/2026-10-01-a".into(), "summary/current".into()]);
        assert!(ledger.record_copy());
        assert_eq!(ledger.copied_through.as_deref(), Some("2026-10-01"));
        assert_eq!(ledger.versions.get("summary/current"), Some(&1));
        assert!(!ledger.record_copy(), "the same copy is not recorded twice");
        ledger.record_prepared(None, vec!["summary/current".into()]);
        assert!(ledger.record_copy());
        assert_eq!(ledger.versions.get("summary/current"), Some(&2), "a replaced document rises one version");
        assert_eq!(ledger.versions.get("days/2026-10-01-a"), Some(&1));
        assert_eq!(ledger.copied_through.as_deref(), Some("2026-10-01"), "a copy with no closed line keeps the day");
    }

    /// Outro endereço é uma página nova com o banco vazio: nenhuma versão
    /// vale, e o que a última preparação levou fica para a cópia seguinte; o
    /// mesmo endereço não mexe em nada.
    #[test]
    fn a_new_address_forgets_the_versions_and_the_same_one_keeps_them() {
        let mut ledger = Ledger::default();
        ledger.record_url("https://claude.ai/a");
        ledger.record_prepared(Some("2026-10-01".into()), vec!["summary/current".into()]);
        assert!(ledger.record_copy());
        ledger.record_url("https://claude.ai/a");
        assert_eq!(ledger.versions.len(), 1);
        assert_eq!(ledger.copied_through.as_deref(), Some("2026-10-01"));
        ledger.record_prepared(Some("2026-10-01".into()), vec!["summary/current".into()]);
        ledger.record_url("https://claude.ai/b");
        assert!(ledger.versions.is_empty() && ledger.copied_through.is_none());
        assert_eq!(ledger.prepared, ["summary/current"], "what was prepared waits for the copy to the new page");
        assert!(ledger.record_copy());
        assert_eq!(ledger.versions.get("summary/current"), Some(&1), "the new database starts at version 1");
    }

    /// Uma linha do dia com `actions` ações e `tokens` tokens, num dia e
    /// projeto.
    fn busy(day: &str, project: &str, actions: u64, tokens: u64) -> DayRow {
        DayRow { tokens, actions, ..row(day, project) }
    }

    /// As médias de três e de sete dias levam os últimos dias fechados com
    /// pelo menos cem ações da máquina inteira e pulam o que teve cinquenta,
    /// mesmo quando ele é o mais novo; os projetos de um dia se somam.
    #[test]
    fn the_averages_skip_a_day_with_fifty_actions_and_add_the_projects_of_a_day() {
        let closed = vec![
            busy("2026-09-25", "a", 150, 1000),
            busy("2026-09-26", "a", 150, 2000),
            busy("2026-09-27", "a", 150, 3000),
            busy("2026-09-28", "a", 150, 4000),
            busy("2026-09-29", "a", 60, 5000),
            busy("2026-09-29", "b", 90, 5000),
            busy("2026-09-30", "a", 150, 6000),
            busy("2026-10-01", "a", 50, 9000),
        ];
        let summary = summarize(&closed, &[], "2026-10-02");
        assert_eq!(summary.last_3, Average { tokens: 6667, days: 3 }, "{summary:?}");
        assert_eq!(summary.last_7, Average { tokens: 4333, days: 6 });
        assert_eq!(summary.yesterday, DayTotal { day: "2026-10-01".into(), tokens: 9000, actions: 50, ..DayTotal::default() });
        assert_eq!(summary.today.day, "2026-10-02");
    }

    /// A média do mês só usa dias do mês do calendário de hoje, e só os que
    /// têm cem ações; dia do mês anterior não entra, mesmo dentro dos sete.
    #[test]
    fn the_month_average_uses_only_busy_days_of_the_current_calendar_month() {
        let closed = vec![
            busy("2026-09-29", "a", 500, 90_000),
            busy("2026-09-30", "a", 500, 90_000),
            busy("2026-10-01", "a", 200, 3000),
            busy("2026-10-02", "a", 40, 99_999),
            busy("2026-10-03", "a", 200, 5000),
        ];
        let summary = summarize(&closed, &[], "2026-10-04");
        assert_eq!(summary.month, Average { tokens: 4000, days: 2 }, "{summary:?}");
        assert_eq!(summary.last_3.days, 3);
        assert_eq!(summarize(&closed, &[], "2026-10-01").month, Average::default(), "the first day of a month has no closed day in it");
    }

    /// Sete dias fechados a 120 milhões de tokens e um último dia a 90
    /// milhões dão menos 25%: o indicador compara o último dia fechado com a
    /// média dos dias de antes, e abaixo da média o sinal é negativo.
    #[test]
    fn a_last_day_of_ninety_million_against_an_average_of_one_hundred_twenty_million_is_minus_twenty_five() {
        let mut closed: Vec<DayRow> = (20..=26).map(|day| busy(&format!("2026-09-{day}"), "a", 300, 120_000_000)).collect();
        closed.push(busy("2026-09-27", "a", 300, 90_000_000));
        let trend = trend(&closed).unwrap();
        assert_eq!(trend.day, "2026-09-27");
        assert_eq!(trend.tokens, 90_000_000);
        assert_eq!(trend.average, Average { tokens: 120_000_000, days: 7 });
        assert_eq!(trend.change_percent, -25);

        let above: Vec<DayRow> = vec![busy("2026-09-26", "a", 300, 100_000), busy("2026-09-27", "a", 300, 112_000)];
        assert_eq!(super::trend(&above).unwrap().change_percent, 12, "above the average is positive");
    }

    /// Um dia com 50 ações fica fora da média, entre os dias de antes do
    /// último: a média é a dos dias de trabalho.
    #[test]
    fn the_trend_average_leaves_out_a_day_with_fifty_actions() {
        let closed = vec![
            busy("2026-09-23", "a", 300, 100),
            busy("2026-09-24", "a", 300, 300),
            busy("2026-09-25", "a", 50, 1_000_000),
            busy("2026-09-26", "a", 100, 400),
        ];
        let trend = trend(&closed).unwrap();
        assert_eq!(trend.average, Average { tokens: 200, days: 2 }, "the 50-action day is not in the average: {trend:?}");
        assert_eq!((trend.day.as_str(), trend.tokens), ("2026-09-26", 400), "a day with exactly 100 actions counts");
        assert_eq!(trend.change_percent, 100);
    }

    /// Sexta com 300 ações, sábado com 5 e domingo com 5: o dia comparado é a
    /// sexta, contra os sete dias de trabalho de antes dela, e os dois dias
    /// parados que vieram depois não entram na conta.
    #[test]
    fn a_quiet_weekend_after_friday_never_becomes_the_compared_day() {
        let mut closed: Vec<DayRow> = (13..=19).map(|day| busy(&format!("2026-09-{day}"), "a", 300, 120_000_000)).collect();
        closed.push(busy("2026-09-20", "a", 300, 90_000_000)); // sexta
        closed.push(busy("2026-09-21", "a", 5, 1_000)); // sábado
        closed.push(busy("2026-09-22", "a", 5, 2_000)); // domingo
        let trend = trend(&closed).unwrap();
        assert_eq!((trend.day.as_str(), trend.tokens), ("2026-09-20", 90_000_000), "{trend:?}");
        assert_eq!(trend.average, Average { tokens: 120_000_000, days: 7 });
        assert_eq!(trend.change_percent, -25, "not the minus 100% of a quiet day");
    }

    /// Só os sete dias fechados anteriores ao último entram na média; o mais
    /// velho de oito fica de fora, e os projetos do último dia se somam.
    #[test]
    fn the_trend_averages_only_the_seven_busy_days_before_the_last_one_and_adds_its_projects() {
        let mut closed = vec![busy("2026-09-10", "a", 300, 8_000_000)];
        closed.extend((11..=17).map(|day| busy(&format!("2026-09-{day}"), "a", 300, 1000)));
        closed.push(busy("2026-09-18", "a", 200, 600));
        closed.push(busy("2026-09-18", "b", 200, 900));
        let trend = trend(&closed).unwrap();
        assert_eq!(trend.average, Average { tokens: 1000, days: 7 }, "the eighth day back is left out");
        assert_eq!(trend.tokens, 1500, "the projects of the last day add up");
        assert_eq!(trend.change_percent, 50);
    }

    /// Sem dia fechado, com um dia de trabalho só, com os dias de antes
    /// parados ou com só dias parados depois do único dia de trabalho, não há
    /// média contra a qual comparar.
    #[test]
    fn the_trend_is_empty_without_a_closed_day_or_a_busy_day_before_the_last() {
        assert_eq!(trend(&[]), None, "no closed day");
        assert_eq!(trend(&[busy("2026-09-27", "a", 300, 5000)]), None, "no day before the last");
        let idle = vec![busy("2026-09-25", "a", 99, 5000), busy("2026-09-26", "a", 10, 5000), busy("2026-09-27", "a", 300, 5000)];
        assert_eq!(trend(&idle), None, "the days before had no work");
        let quiet_after = vec![busy("2026-09-26", "a", 300, 5000), busy("2026-09-27", "a", 5, 5000)];
        assert_eq!(trend(&quiet_after), None, "the only workday has no day before it");
        assert_eq!(trend(&[busy("2026-09-27", "a", 5, 5000)]), None, "no workday at all");
    }

    /// Meio ponto arredonda para longe do zero, nos dois lados.
    #[test]
    fn the_trend_rounds_half_a_point_away_from_zero() {
        let around = |last: u64| {
            trend(&[busy("2026-09-26", "a", 300, 200), busy("2026-09-27", "a", 300, last)]).unwrap().change_percent
        };
        assert_eq!((around(175), around(225), around(201), around(199)), (-13, 13, 1, -1));
    }

    /// Hoje só entra em "hoje até agora", com as linhas do dia aberto
    /// somadas; ele não puxa média nem previsão, mesmo quando vem junto das
    /// linhas fechadas.
    #[test]
    fn today_is_an_open_day_that_only_shows_so_far() {
        let open = vec![
            DayRow { partial: true, ..busy("2026-10-02", "a", 300, 7000) },
            DayRow { partial: true, ..busy("2026-10-02", "b", 100, 500) },
        ];
        let closed = vec![busy("2026-10-01", "a", 200, 1000), busy("2026-10-02", "a", 999, 999_999)];
        let summary = summarize(&closed, &open, "2026-10-02");
        assert_eq!((summary.today.tokens, summary.today.actions), (7500, 400));
        assert_eq!(summary.last_3, Average { tokens: 1000, days: 1 }, "today is not in the average");
        assert_eq!(summary.month, Average { tokens: 1000, days: 1 });
    }

    /// Dez dias fechados a US$ 0,02 cada, num mês de 31 dias com hoje no dia
    /// 11, dão US$ 0,62 de previsão: US$ 0,20 já gastos mais US$ 0,02 pelos
    /// 21 dias que faltam, hoje incluído. Em tokens, a mesma conta.
    #[test]
    fn the_jev_forecast_of_ten_closed_days_at_two_cents_in_a_31_day_month_is_sixty_two_cents() {
        let closed: Vec<DayRow> = (1..=10)
            .map(|day| DayRow {
                mustard_searches: 1,
                jev_cost_micro_usd: 20_000,
                jev_tokens: 700,
                ..busy(&format!("2026-10-{day:02}"), "a", 10, 1)
            })
            .collect();
        let forecast = summarize(&closed, &[], "2026-10-11").forecast;
        assert_eq!(forecast.days_left, 21);
        assert_eq!(forecast.spent_micro_usd, 200_000);
        assert_eq!(forecast.daily_micro_usd, 20_000);
        assert_eq!(forecast.micro_usd, 620_000, "US$ 0,62");
        assert_eq!((forecast.spent_tokens, forecast.daily_tokens, forecast.tokens), (7000, 700, 7000 + 700 * 21));
    }

    /// A média da previsão leva os últimos sete dias com busca, de qualquer
    /// mês, e pula os sem busca; sem nenhum dia com busca, a previsão é o que
    /// já foi gasto.
    #[test]
    fn the_forecast_averages_the_last_seven_days_with_a_search_and_skips_the_others() {
        let mut closed: Vec<DayRow> = (20_u64..=29)
            .map(|day| DayRow { mustard_searches: 1, jev_cost_micro_usd: 1000 * day, ..busy(&format!("2026-09-{day}"), "a", 10, 1) })
            .collect();
        closed.push(busy("2026-09-30", "a", 500, 1));
        let forecast = summarize(&closed, &[], "2026-10-01").forecast;
        assert_eq!(forecast.daily_micro_usd, (23 + 24 + 25 + 26 + 27 + 28 + 29) * 1000 / 7, "the last seven with a search");
        assert_eq!((forecast.spent_micro_usd, forecast.days_left), (0, 31));
        let quiet = summarize(&[busy("2026-09-30", "a", 500, 1)], &[], "2026-10-05").forecast;
        assert_eq!((quiet.micro_usd, quiet.daily_micro_usd), (0, 0));
        assert_eq!(days_left_in_month("2026-12-31"), 1);
        assert_eq!(days_left_in_month("2026-02-01"), 28);
    }

    /// A busca por comando e a chamada que ela grava na spec são a mesma
    /// busca: cada chamada leva o uso mais próximo antes dela, dentro da
    /// duração da busca mais a folga, e cada uso pareia uma vez só; a chamada
    /// de antes do uso, a longe e a que sobra não pareiam.
    #[test]
    fn a_command_search_pairs_with_the_event_it_recorded_once() {
        let at = |text: &str| DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc);
        let start = at("2026-10-01T15:00:00Z");
        let paired = |commands: &[DateTime<Utc>], events: &[(DateTime<Utc>, u64)]| paired_searches(commands, events);

        assert_eq!(paired(&[start], &[(at("2026-10-01T15:00:03Z"), 1500)]), [0], "the event closes seconds after the command");
        assert_eq!(paired(&[start], &[(at("2026-10-01T15:00:00Z"), 1500)]), [0], "the event stamp has no fraction of a second");
        assert_eq!(paired(&[at("2026-10-01T15:00:00.800Z")], &[(at("2026-10-01T15:00:00Z"), 100)]), [0], "a second of rounding is allowed");
        assert_eq!(paired(&[start], &[(at("2026-10-01T15:00:03Z"), 1500), (at("2026-10-01T15:00:04Z"), 1500)]), [0], "one command pairs with one event");
        assert_eq!(
            paired(&[start, at("2026-10-01T15:00:10Z")], &[(at("2026-10-01T15:00:12Z"), 500)]),
            [1],
            "the nearest command before the event"
        );
        assert_eq!(paired(&[start], &[(at("2026-10-01T15:10:00Z"), 1000)]), Vec::<usize>::new(), "an event ten minutes later is another search");
        assert_eq!(paired(&[start], &[(at("2026-10-01T14:59:50Z"), 1000)]), Vec::<usize>::new(), "an event before the command is not its event");
        assert_eq!(paired(&[], &[(start, 10)]), Vec::<usize>::new(), "an event with no command stays alone");
        assert_eq!(paired(&[start], &[]), Vec::<usize>::new(), "a command with no event stays alone");
    }
}
