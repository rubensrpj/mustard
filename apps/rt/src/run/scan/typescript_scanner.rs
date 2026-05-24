//! TypeScript / Node.js stack scanner — a port of
//! `registry/scanners/typescript-scanner.js`.
//!
//! Detects Drizzle, Prisma and TypeORM entities plus TypeScript enums. The JS
//! scanner used regular expressions; the extraction here is rewritten with
//! hand-written string scanning that preserves the same decision logic.

use super::file_utils::{collect_files, infer_common_folder, read_file_safe, relative_path};
use mustard_core::fs;
use super::rust_scanner::extract_brace_body;
use super::{
    detect_value_convention, DtoInfo, EndpointInfo, EntityInfo, EnumInfo, RouteInfo, ScanResult,
    Scanner, ServiceInfo,
};
use std::collections::BTreeMap;
use std::path::Path;

/// TypeScript scanner — selected when `package.json` / `tsconfig.json` is present.
pub struct TypeScriptScanner;

/// Frameworks detected from `package.json` dependency keys.
struct Frameworks {
    drizzle: bool,
    prisma: bool,
    typeorm: bool,
    nestjs: bool,
    express: bool,
    hono: bool,
    nextjs: bool,
    zod: bool,
    class_validator: bool,
}

/// `true` if `root` has a `package.json` declaring `dep` as a (dev)dependency.
fn package_has_dep(root: &Path, dep: &str) -> bool {
    let Some(content) = read_file_safe(&root.join("package.json")) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let in_section = |section: &str| {
        json.get(section)
            .and_then(serde_json::Value::as_object)
            .is_some_and(|obj| obj.contains_key(dep))
    };
    in_section("dependencies") || in_section("devDependencies")
}

/// Uppercase the first ASCII character — a port of `_toPascalCase`.
fn to_pascal_case(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => name.to_string(),
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

impl TypeScriptScanner {
    fn detect_frameworks(root: &Path) -> Frameworks {
        Frameworks {
            drizzle: package_has_dep(root, "drizzle-orm") || package_has_dep(root, "drizzle-kit"),
            prisma: package_has_dep(root, "@prisma/client")
                || package_has_dep(root, "prisma")
                || root.join("prisma/schema.prisma").exists(),
            typeorm: package_has_dep(root, "typeorm"),
            nestjs: package_has_dep(root, "@nestjs/core")
                || package_has_dep(root, "@nestjs/common"),
            express: package_has_dep(root, "express"),
            hono: package_has_dep(root, "hono"),
            nextjs: package_has_dep(root, "next")
                || root.join("next.config.js").exists()
                || root.join("next.config.ts").exists()
                || root.join("next.config.mjs").exists(),
            zod: package_has_dep(root, "zod"),
            class_validator: package_has_dep(root, "class-validator"),
        }
    }

    /// Drizzle: `export const tableVar = pgTable('table_name', { … })`.
    fn scan_drizzle(root: &Path, entities: &mut BTreeMap<String, EntityInfo>) {
        for file in collect_files(root, ".ts", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            if !content.contains("pgTable") {
                continue;
            }
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("export const ") {
                let idx = search + rel_idx;
                search = idx + "export const ".len();
                let after = &content[search..];
                let var: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if var.is_empty() {
                    continue;
                }
                let rest = after[var.len()..].trim_start();
                if !rest.starts_with("= pgTable") && !rest.starts_with("=pgTable") {
                    continue;
                }
                let brace = match content[idx..].find('{') {
                    Some(b) => idx + b,
                    None => continue,
                };
                let props = extract_brace_body(&content, brace)
                    .map(|body| {
                        body.lines()
                            .filter_map(|l| {
                                let t = l.trim();
                                let ident: String = t
                                    .chars()
                                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                                    .collect();
                                if !ident.is_empty()
                                    && t[ident.len()..].trim_start().starts_with(':')
                                {
                                    Some(ident)
                                } else {
                                    None
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                entities.insert(
                    to_pascal_case(&var),
                    EntityInfo {
                        file: rel.clone(),
                        decorators: vec!["pgTable".to_string()],
                        properties: props,
                        ..EntityInfo::default()
                    },
                );
            }
        }
    }

    /// Prisma: `model Name { … }` declarations in `prisma/schema.prisma`.
    fn scan_prisma(root: &Path, entities: &mut BTreeMap<String, EntityInfo>) {
        let schema = root.join("prisma/schema.prisma");
        let Some(content) = read_file_safe(&schema) else {
            return;
        };
        let rel = relative_path(root, &schema);
        let mut search = 0;
        while let Some(rel_idx) = content[search..].find("model ") {
            let idx = search + rel_idx;
            // Must be at line start (ignore leading whitespace).
            let line_start = content[..idx].rfind('\n').map_or(0, |n| n + 1);
            if content[line_start..idx].trim().is_empty() {
                let after = &content[idx + "model ".len()..];
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    if let Some(brace) = content[idx..].find('{') {
                        let props = extract_brace_body(&content, idx + brace)
                            .map(|body| {
                                body.lines()
                                    .filter_map(|l| {
                                        let f: String = l
                                            .trim()
                                            .chars()
                                            .take_while(|c| {
                                                c.is_ascii_alphanumeric() || *c == '_'
                                            })
                                            .collect();
                                        (!f.is_empty() && !f.starts_with('@')).then_some(f)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        entities.insert(
                            name,
                            EntityInfo {
                                file: rel.clone(),
                                decorators: vec!["prisma-model".to_string()],
                                properties: props,
                                ..EntityInfo::default()
                            },
                        );
                    }
                }
            }
            search = idx + "model ".len();
        }
    }

    /// TypeORM: classes preceded by an `@Entity(...)` decorator.
    fn scan_typeorm(root: &Path, entities: &mut BTreeMap<String, EntityInfo>) {
        for file in collect_files(root, ".ts", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            if !content.contains("@Entity") {
                continue;
            }
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("@Entity") {
                let idx = search + rel_idx;
                search = idx + "@Entity".len();
                if let Some(class_off) = content[idx..].find("class ") {
                    let after = &content[idx + class_off + "class ".len()..];
                    let name: String = after
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        entities.entry(name).or_insert_with(|| EntityInfo {
                            file: rel.clone(),
                            decorators: vec!["@Entity".to_string()],
                            ..EntityInfo::default()
                        });
                    }
                }
            }
        }
    }

    /// Scan an identifier (alphanumeric + `_`) starting at byte offset `start`.
    fn ident_at(content: &str, start: usize) -> String {
        content[start..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect()
    }

    /// Routes — a port of `scanRoutes()` (Next.js / NestJS / Express+Hono).
    fn scan_routes_impl(root: &Path, fw: &Frameworks) -> BTreeMap<String, RouteInfo> {
        let mut routes = BTreeMap::new();
        if fw.nextjs {
            Self::scan_nextjs_routes(root, &mut routes);
        }
        if fw.nestjs {
            Self::scan_nestjs_routes(root, &mut routes);
        }
        if fw.express || fw.hono {
            Self::scan_express_hono_routes(root, &mut routes);
        }
        routes
    }

    /// Next.js App Router — `route.ts` files, path inferred from the directory.
    fn scan_nextjs_routes(root: &Path, routes: &mut BTreeMap<String, RouteInfo>) {
        const METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
        let app_dir = if root.join("app").is_dir() {
            root.join("app")
        } else if root.join("src/app").is_dir() {
            root.join("src/app")
        } else {
            return;
        };
        let mut files = collect_files(&app_dir, ".ts", &[]);
        files.extend(collect_files(&app_dir, ".tsx", &[]));
        for file in files {
            let base = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if base != "route.ts" && base != "route.tsx" {
                continue;
            }
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            let dir = file.parent().unwrap_or(&app_dir);
            let route_path = format!(
                "/{}",
                dir.strip_prefix(&app_dir)
                    .unwrap_or(dir)
                    .to_string_lossy()
                    .replace('\\', "/")
            );
            // Detect exported HTTP-method handlers: `export [async] [function] METHOD( | {`.
            let mut endpoints = Vec::new();
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("export ") {
                let idx = search + rel_idx;
                search = idx + "export ".len();
                let mut rest = content[search..].trim_start();
                rest = rest.strip_prefix("async ").unwrap_or(rest).trim_start();
                rest = rest.strip_prefix("function ").unwrap_or(rest).trim_start();
                let ident: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if METHODS.contains(&ident.as_str()) {
                    let after = rest[ident.len()..].trim_start();
                    if after.starts_with('(') || after.starts_with('{') {
                        endpoints.push(EndpointInfo {
                            method: ident.clone(),
                            path: route_path.clone(),
                            name: Some(ident),
                        });
                    }
                }
            }
            if !endpoints.is_empty() {
                let key = if route_path.is_empty() {
                    "/".to_string()
                } else {
                    route_path.clone()
                };
                routes.insert(
                    key,
                    RouteInfo {
                        file: relative_path(root, &file),
                        prefix: route_path,
                        endpoints,
                    },
                );
            }
        }
    }

    /// NestJS — `@Controller(...)` files with `@Get/@Post/...` method decorators.
    fn scan_nestjs_routes(root: &Path, routes: &mut BTreeMap<String, RouteInfo>) {
        const METHODS: &[&str] = &["Get", "Post", "Put", "Delete", "Patch", "Head", "Options"];
        for file in collect_files(root, ".ts", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            if !content.contains("@Controller") {
                continue;
            }
            let rel = relative_path(root, &file);
            let mut csearch = 0;
            while let Some(rel_idx) = content[csearch..].find("@Controller") {
                let cidx = csearch + rel_idx;
                csearch = cidx + "@Controller".len();
                let prefix = Self::decorator_string_arg(&content[cidx..])
                    .map_or_else(|| "/".to_string(), |p| format!("/{}", p.trim_start_matches('/')));
                let after_ctrl = &content[cidx..];
                let mut endpoints = Vec::new();
                let mut msearch = 0;
                while msearch < after_ctrl.len() {
                    let Some(rel_m) = after_ctrl[msearch..].find('@') else {
                        break;
                    };
                    let midx = msearch + rel_m;
                    msearch = midx + 1;
                    let method_name = Self::ident_at(after_ctrl, midx + 1);
                    if !METHODS.contains(&method_name.as_str()) {
                        continue;
                    }
                    let arg_start = midx + 1 + method_name.len();
                    let sub_path = Self::decorator_string_arg(&after_ctrl[arg_start..])
                        .filter(|s| !s.is_empty())
                        .map(|s| format!("/{}", s.trim_start_matches('/')));
                    let full_path = match &sub_path {
                        Some(s) => format!("{prefix}{s}"),
                        None => prefix.clone(),
                    };
                    endpoints.push(EndpointInfo {
                        method: method_name.to_uppercase(),
                        path: full_path,
                        name: None,
                    });
                }
                if !endpoints.is_empty() {
                    routes.insert(
                        format!("{prefix}:{rel}"),
                        RouteInfo {
                            file: rel.clone(),
                            prefix,
                            endpoints,
                        },
                    );
                }
            }
        }
    }

    /// Extract the first single/double-quoted string argument of a `@Decorator(...)`.
    fn decorator_string_arg(slice: &str) -> Option<String> {
        let open = slice.find('(')?;
        let close = slice[open..].find(')')? + open;
        let inner = slice[open + 1..close].trim();
        let inner = inner
            .strip_prefix('\'')
            .or_else(|| inner.strip_prefix('"'))?;
        let end = inner.find(['\'', '"'])?;
        Some(inner[..end].to_string())
    }

    /// Express / Hono — `router.get('/path', ...)` / `app.post(...)` calls.
    fn scan_express_hono_routes(root: &Path, routes: &mut BTreeMap<String, RouteInfo>) {
        const VERBS: &[&str] = &["get", "post", "put", "delete", "patch"];
        let mut files = collect_files(root, ".ts", &[]);
        files.extend(collect_files(root, ".js", &[]));
        for file in files {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            if !content.contains("router.") && !content.contains("app.") {
                continue;
            }
            let rel = relative_path(root, &file);
            let mut endpoints = Vec::new();
            for prefix_tok in ["router.", "app."] {
                let mut search = 0;
                while let Some(rel_idx) = content[search..].find(prefix_tok) {
                    let idx = search + rel_idx;
                    search = idx + prefix_tok.len();
                    let verb = Self::ident_at(&content, idx + prefix_tok.len());
                    if !VERBS.contains(&verb.as_str()) {
                        continue;
                    }
                    let rest = content[idx + prefix_tok.len() + verb.len()..].trim_start();
                    if let Some(path) = Self::decorator_string_arg(rest) {
                        endpoints.push(EndpointInfo {
                            method: verb.to_uppercase(),
                            path,
                            name: None,
                        });
                    }
                }
            }
            if !endpoints.is_empty() {
                let prefix =
                    infer_route_prefix(&endpoints.iter().map(|e| e.path.clone()).collect::<Vec<_>>());
                routes.insert(
                    rel.clone(),
                    RouteInfo {
                        file: rel,
                        prefix,
                        endpoints,
                    },
                );
            }
        }
    }

    /// DTOs — a port of `scanDtos()`: `*Dto/Request/Response/Input/Output`
    /// classes/interfaces/types and `z.`-backed schema consts.
    fn scan_dtos_impl(root: &Path, fw: &Frameworks) -> BTreeMap<String, DtoInfo> {
        const DTO_SUFFIXES: &[&str] = &["Dto", "Request", "Response", "Input", "Output"];
        let mut dtos = BTreeMap::new();
        for file in collect_files(root, ".ts", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            let rel = relative_path(root, &file);
            let validation_pattern = if fw.zod && content.contains("z.") {
                "zod"
            } else if fw.class_validator && content.contains("@Is") {
                "class-validator"
            } else {
                "none"
            };
            // `export (class|interface|type) Name` where Name ends with a DTO suffix.
            for kw in ["export class ", "export interface ", "export type "] {
                let mut search = 0;
                while let Some(rel_idx) = content[search..].find(kw) {
                    let idx = search + rel_idx;
                    search = idx + kw.len();
                    let name = Self::ident_at(&content, idx + kw.len());
                    if name.is_empty() {
                        continue;
                    }
                    if let Some(suffix) = DTO_SUFFIXES.iter().find(|s| name.ends_with(*s)) {
                        let stem = name[..name.len() - suffix.len()].to_string();
                        let entity = (!stem.is_empty() && stem != name).then_some(stem);
                        dtos.insert(
                            name,
                            DtoInfo {
                                file: rel.clone(),
                                entity,
                                validation_pattern: validation_pattern.to_string(),
                            },
                        );
                    }
                }
            }
            // Zod schemas: `export const NameSchema = z.` (also Input/Output stems).
            if fw.zod {
                let mut search = 0;
                while let Some(rel_idx) = content[search..].find("export const ") {
                    let idx = search + rel_idx;
                    search = idx + "export const ".len();
                    let name = Self::ident_at(&content, idx + "export const ".len());
                    if name.is_empty()
                        || !(name.ends_with("Schema")
                            || name.ends_with("Input")
                            || name.ends_with("Output"))
                    {
                        continue;
                    }
                    let rest = content[idx + "export const ".len() + name.len()..].trim_start();
                    let rest = rest.strip_prefix('=').map_or(rest, str::trim_start);
                    if rest.starts_with("z.") {
                        dtos.insert(
                            name,
                            DtoInfo {
                                file: rel.clone(),
                                entity: None,
                                validation_pattern: "zod".to_string(),
                            },
                        );
                    }
                }
            }
        }
        dtos
    }

    /// Services — a port of `scanServices()`: `export class *Service` (NestJS
    /// `@Injectable` services are also caught by the generic export scan).
    fn scan_services_impl(root: &Path) -> BTreeMap<String, ServiceInfo> {
        let mut services = BTreeMap::new();
        for file in collect_files(root, ".ts", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            if !content.contains("Service") {
                continue;
            }
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("export class ") {
                let idx = search + rel_idx;
                search = idx + "export class ".len();
                let name = Self::ident_at(&content, idx + "export class ".len());
                if name.ends_with("Service") && name.len() > "Service".len() {
                    let entity = name[..name.len() - "Service".len()].to_string();
                    services.entry(name).or_insert(ServiceInfo {
                        file: rel.clone(),
                        entity: Some(entity),
                        dependencies: Vec::new(),
                    });
                }
            }
        }
        services
    }
}

/// Infer a shared route prefix from a list of paths — a port of `_inferRoutePrefix`.
fn infer_route_prefix(paths: &[String]) -> String {
    let Some(first) = paths.first() else {
        return "/".to_string();
    };
    let mut common = String::new();
    for segment in first.split('/').filter(|s| !s.is_empty()) {
        let candidate = format!("/{segment}");
        if paths.iter().all(|p| p.starts_with(&candidate)) {
            common.push_str(&candidate);
        } else {
            break;
        }
    }
    if common.is_empty() {
        "/".to_string()
    } else {
        common
    }
}

/// Most common element of a slice — a port of `_mostCommon`.
fn most_common(items: &[String]) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for item in items {
        if let Some(e) = counts.iter_mut().find(|(v, _)| v == item) {
            e.1 += 1;
        } else {
            counts.push((item.clone(), 1));
        }
    }
    counts.into_iter().max_by_key(|(_, c)| *c).map(|(v, _)| v)
}

/// Naming convention of a list of identifiers — a port of `_detectNamingConvention`.
fn detect_naming_convention(names: &[String]) -> String {
    if names.is_empty() {
        return "PascalCase".to_string();
    }
    let pascal = names
        .iter()
        .filter(|n| {
            n.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && n.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .count();
    let camel = names
        .iter()
        .filter(|n| {
            n.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                && n.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .count();
    if pascal >= camel {
        "PascalCase".to_string()
    } else {
        "camelCase".to_string()
    }
}

/// Shared route prefix across paths — a port of `_detectCommonPrefix`.
fn detect_common_prefix(paths: &[String]) -> String {
    let firsts: Vec<String> = paths
        .iter()
        .filter_map(|p| {
            p.trim_start_matches('/')
                .split('/')
                .next()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .collect();
    match most_common(&firsts) {
        Some(seg) => format!("/{seg}"),
        None => "/".to_string(),
    }
}

/// Route naming pattern — a port of `_detectRouteNamingPattern`.
fn detect_route_naming_pattern(paths: &[String]) -> String {
    if paths.is_empty() {
        return "unknown".to_string();
    }
    let segments: Vec<&str> = paths
        .iter()
        .flat_map(|p| p.split('/'))
        .filter(|s| !s.is_empty() && !s.starts_with(':') && !s.starts_with('{'))
        .collect();
    if segments.is_empty() {
        return "unknown".to_string();
    }
    let is_kebab = |s: &str| {
        s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    let is_camel = |s: &str| {
        s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && s.chars().all(|c| c.is_ascii_alphanumeric())
            && s.chars().any(|c| c.is_ascii_uppercase())
    };
    let total = segments.len() as f64;
    let kebab = segments.iter().filter(|s| is_kebab(s)).count() as f64;
    let camel = segments.iter().filter(|s| is_camel(s)).count() as f64;
    if kebab / total > 0.5 {
        "kebab-case".to_string()
    } else if camel / total > 0.5 {
        "camelCase".to_string()
    } else {
        "mixed".to_string()
    }
}

/// Convert a `serde_json::Value` of `null`/`String` for the optional folder key.
fn folder_value(folder: Option<String>) -> serde_json::Value {
    match folder {
        Some(f) => serde_json::Value::String(f),
        None => serde_json::Value::Null,
    }
}

impl Scanner for TypeScriptScanner {
    fn detect(&self, root: &Path) -> bool {
        root.join("package.json").exists() || root.join("tsconfig.json").exists()
    }

    fn detect_architecture(&self, root: &Path) -> String {
        let fw = Self::detect_frameworks(root);
        let list_dirs = |dir: &Path| -> Vec<String> {
            fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .into_iter()
                        .filter(|e| e.is_dir)
                        .map(|e| e.file_name)
                        .collect()
                })
                .unwrap_or_default()
        };
        // Plain service/repository separation with interfaces → `solid`.
        let has_svc_repo = {
            let base = if root.join("src").is_dir() {
                root.join("src")
            } else {
                root.to_path_buf()
            };
            let dirs: Vec<String> = list_dirs(&base).iter().map(|d| d.to_lowercase()).collect();
            let has = |n: &str| dirs.iter().any(|d| d == n);
            (has("services") || has("service"))
                && (has("repositories")
                    || has("repository")
                    || has("repos")
                    || has("interfaces")
                    || has("contracts"))
        };
        if fw.nestjs && root.join("src").is_dir() {
            return "solid".to_string();
        }
        if !fw.nestjs && has_svc_repo {
            return "solid".to_string();
        }
        if fw.nextjs {
            if root.join("app").is_dir() || root.join("src/app").is_dir() {
                return "feature-based".to_string();
            }
            if root.join("pages").is_dir() || root.join("src/pages").is_dir() {
                return "pages-based".to_string();
            }
        }
        // React/frontend atomic structure.
        if root.join("src/components").is_dir() && root.join("src/hooks").is_dir() {
            let looks_feature = list_dirs(&root.join("src/components"))
                .iter()
                .any(|d| d.chars().next().is_some_and(|c| c.is_ascii_uppercase()));
            return if looks_feature {
                "feature-based".to_string()
            } else {
                "atomic".to_string()
            };
        }
        let src_dirs = if root.join("src").is_dir() {
            list_dirs(&root.join("src"))
        } else {
            list_dirs(root)
        };
        if src_dirs.len() <= 2 {
            "minimal".to_string()
        } else {
            "layered".to_string()
        }
    }

    fn scan_entities(&self, root: &Path) -> BTreeMap<String, EntityInfo> {
        let mut entities = BTreeMap::new();
        let fw = Self::detect_frameworks(root);
        if fw.drizzle {
            Self::scan_drizzle(root, &mut entities);
        }
        if fw.prisma {
            Self::scan_prisma(root, &mut entities);
        }
        if fw.typeorm {
            Self::scan_typeorm(root, &mut entities);
        }
        entities
    }

    fn scan_enums(&self, root: &Path) -> BTreeMap<String, EnumInfo> {
        let mut enums = BTreeMap::new();
        for file in collect_files(root, ".ts", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            if !content.contains("enum ") {
                continue;
            }
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("enum ") {
                let idx = search + rel_idx;
                search = idx + "enum ".len();
                // Require an `export` (optionally `export const`) before `enum`.
                let before = content[..idx].trim_end();
                if !before.ends_with("export") && !before.ends_with("export const") {
                    continue;
                }
                let after = &content[idx + "enum ".len()..];
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name.is_empty() {
                    continue;
                }
                let Some(brace) = content[idx..].find('{') else {
                    continue;
                };
                let Some(body) = extract_brace_body(&content, idx + brace) else {
                    continue;
                };
                let values: Vec<String> = body
                    .split([',', '\n'])
                    .filter_map(|raw| {
                        let member = raw.trim();
                        let name = member.split('=').next().unwrap_or("").trim();
                        (!name.is_empty()
                            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                        .then(|| name.to_string())
                    })
                    .collect();
                let convention = detect_value_convention(&values);
                enums.insert(
                    name,
                    EnumInfo {
                        values,
                        file: rel.clone(),
                        decorators: Vec::new(),
                        value_convention: Some(convention),
                    },
                );
            }
        }
        enums
    }

    fn scan_routes(&self, root: &Path) -> BTreeMap<String, RouteInfo> {
        Self::scan_routes_impl(root, &Self::detect_frameworks(root))
    }

    fn scan_dtos(&self, root: &Path) -> BTreeMap<String, DtoInfo> {
        Self::scan_dtos_impl(root, &Self::detect_frameworks(root))
    }

    fn scan_services(&self, root: &Path) -> BTreeMap<String, ServiceInfo> {
        Self::scan_services_impl(root)
    }

    /// Infer the `_patterns.typescript` object — a port of `inferPatterns()`.
    fn infer_patterns(&self, root: &Path, result: &ScanResult) -> serde_json::Value {
        let fw = Self::detect_frameworks(root);

        let orm = if fw.drizzle {
            "drizzle"
        } else if fw.prisma {
            "prisma"
        } else if fw.typeorm {
            "typeorm"
        } else {
            "none"
        };
        let framework = if fw.nestjs {
            "nestjs"
        } else if fw.nextjs {
            "nextjs"
        } else if fw.express {
            "express"
        } else if fw.hono {
            "hono"
        } else {
            "none"
        };

        // Entity patterns.
        let entity_files: Vec<String> = result
            .entities
            .values()
            .map(|e| e.file.clone())
            .filter(|f| !f.is_empty())
            .collect();
        let entity_folder = infer_common_folder(&entity_files);
        let def_style = if fw.drizzle {
            "pgTable"
        } else if fw.typeorm {
            "decorator"
        } else if fw.prisma {
            "prisma-model"
        } else {
            "none"
        };
        let entity_naming =
            detect_naming_convention(&result.entities.keys().cloned().collect::<Vec<_>>());

        // Enum patterns.
        let enum_files: Vec<String> = result
            .enums
            .values()
            .map(|e| e.file.clone())
            .filter(|f| !f.is_empty())
            .collect();
        let enum_folder = infer_common_folder(&enum_files);
        let enum_styles: Vec<String> = result
            .enums
            .values()
            .filter_map(|e| e.decorators.first().cloned())
            .collect();
        let enum_def_style = most_common(&enum_styles).unwrap_or_else(|| "ts-enum".to_string());
        let all_enum_values: Vec<String> = result
            .enums
            .values()
            .flat_map(|e| e.values.clone())
            .collect();
        let enum_value_convention = detect_value_convention(&all_enum_values);
        let enum_separate_files = enum_folder.is_some() && enum_folder != entity_folder;

        // Route patterns.
        let route_style = if fw.nextjs {
            "file-based"
        } else if fw.nestjs {
            "decorator"
        } else if fw.express || fw.hono {
            "minimal-api"
        } else {
            "none"
        };
        let all_prefixes: Vec<String> = result
            .routes
            .values()
            .map(|r| r.prefix.clone())
            .filter(|p| !p.is_empty())
            .collect();
        let route_prefix = detect_common_prefix(&all_prefixes);
        let route_paths: Vec<String> = result
            .routes
            .values()
            .flat_map(|r| r.endpoints.iter().map(|e| e.path.clone()))
            .filter(|p| !p.is_empty())
            .collect();
        let route_naming = detect_route_naming_pattern(&route_paths);

        // DTO patterns.
        let dto_files: Vec<String> = result
            .dtos
            .values()
            .map(|d| d.file.clone())
            .filter(|f| !f.is_empty())
            .collect();
        let dto_folder = infer_common_folder(&dto_files);
        let validation_tool = if fw.zod {
            "zod".to_string()
        } else if fw.class_validator {
            "class-validator".to_string()
        } else {
            let patterns: Vec<String> = result
                .dtos
                .values()
                .map(|d| d.validation_pattern.clone())
                .filter(|p| !p.is_empty() && p != "none")
                .collect();
            most_common(&patterns).unwrap_or_else(|| "none".to_string())
        };

        serde_json::json!({
            "orm": orm,
            "framework": framework,
            "entity": {
                "folder": folder_value(entity_folder),
                "defStyle": def_style,
                "namingConvention": entity_naming,
            },
            "enum": {
                "folder": folder_value(enum_folder),
                "defStyle": enum_def_style,
                "separateFiles": enum_separate_files,
                "valueConvention": enum_value_convention,
            },
            "routes": {
                "style": route_style,
                "prefix": route_prefix,
                "namingPattern": route_naming,
            },
            "dto": {
                "folder": folder_value(dto_folder),
                "validationTool": validation_tool,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_requires_manifest() {
        let dir = tempdir().unwrap();
        assert!(!TypeScriptScanner.detect(dir.path()));
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        assert!(TypeScriptScanner.detect(dir.path()));
    }

    #[test]
    fn scan_entities_extracts_drizzle_table() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"drizzle-orm":"0.1"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("schema.ts"),
            "export const users = pgTable('users', {\n  id: serial(),\n  name: text(),\n});\n",
        )
        .unwrap();
        let entities = TypeScriptScanner.scan_entities(dir.path());
        let users = entities.get("Users").expect("Users entity");
        assert_eq!(users.decorators, vec!["pgTable".to_string()]);
        assert!(users.properties.contains(&"id".to_string()));
        assert!(users.properties.contains(&"name".to_string()));
    }

    #[test]
    fn scan_enums_extracts_ts_enum() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            dir.path().join("status.ts"),
            "export enum Status {\n  Active = 'active',\n  Closed = 'closed',\n}\n",
        )
        .unwrap();
        let enums = TypeScriptScanner.scan_enums(dir.path());
        let status = enums.get("Status").expect("Status enum");
        assert_eq!(status.values, vec!["Active", "Closed"]);
    }
}
