//! Os itens de um objeto de constantes no mapa: a chave de cada par de
//! `export const ROTAS = { home: '/' }` é um membro (`enum_member`), como o
//! item de um enum, e o comentário escrito em cima dela é a documentação dele.
//! O objeto escrito dentro de uma função, como argumento de chamada ou como
//! valor de retorno não entra. A varredura é a de verdade, num projeto de
//! mentira gravado em disco, e a conferência é feita no mapa gravado.

#[path = "support/model.rs"]
mod model;

use std::path::{Path, PathBuf};

use serde_json::Value;

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// Um projeto de mentira com os arquivos dados, varrido: o mapa gravado.
fn scanned(name: &str, files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf, Value) {
    let temp = tempfile::Builder::new().prefix(&format!("scan-{name}-")).tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    for (rel, body) in files {
        write(&dir, rel, body);
    }
    let map = model::scan(&dir, &dir.join(".claude"), &[]).0;
    (temp, dir, map)
}

/// As declarações do arquivo `file` como o mapa as gravou.
fn declarations<'a>(map: &'a Value, file: &str) -> &'a [Value] {
    map["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["path"] == file)
        .unwrap_or_else(|| panic!("{file} não está no mapa: {map}"))["declarations"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
}

/// Os nomes das declarações de tipo `kind` do arquivo `file`, em ordem.
fn names_of(map: &Value, file: &str, kind: &str) -> Vec<String> {
    declarations(map, file)
        .iter()
        .filter(|d| d["kind"] == kind)
        .map(|d| d["name"].as_str().unwrap().to_string())
        .collect()
}

/// A declaração `name` de tipo `enum_member` do arquivo `file`.
fn member<'a>(map: &'a Value, file: &str, name: &str) -> &'a Value {
    declarations(map, file)
        .iter()
        .find(|d| d["name"] == name && d["kind"] == "enum_member")
        .unwrap_or_else(|| panic!("{name} não é membro em {file}: {:?}", declarations(map, file)))
}

fn doc_of(decl: &Value) -> &str {
    decl.get("doc").and_then(Value::as_str).unwrap_or("")
}

const ROUTES: &str = "\
// Rotas da loja.
export const ROUTES = {
  // Página inicial da loja.
  home: '/',
  /** Carrinho de compras do cliente. */
  cart: '/carrinho',

  // Solto do par de baixo pela linha em branco.

  orphan: '/solto',
  'admin-users': '/admin/usuarios',
};
";

#[test]
fn an_exported_object_files_each_pair_as_a_member_with_the_comment_above_it_as_doc() {
    let (_temp, _dir, map) = scanned("objeto-exportado", &[("src/routes.ts", ROUTES)]);

    // A constante continua sendo a declaração do objeto, com o comentário dela.
    let routes = declarations(&map, "src/routes.ts").iter().find(|d| d["name"] == "ROUTES").unwrap();
    assert_eq!(routes["kind"], "const", "{routes}");
    assert_eq!(doc_of(routes), "Rotas da loja.", "{routes}");

    assert_eq!(
        names_of(&map, "src/routes.ts", "enum_member"),
        vec!["home", "cart", "orphan", "admin-users"],
        "uma declaração por chave, a de texto entre aspas sem as aspas"
    );
    assert_eq!(doc_of(member(&map, "src/routes.ts", "home")), "Página inicial da loja.");
    assert_eq!(doc_of(member(&map, "src/routes.ts", "cart")), "Carrinho de compras do cliente.");
    // A linha em branco entre o comentário e o par o solta da documentação, e
    // o par sem comentário fica sem documentação.
    assert_eq!(doc_of(member(&map, "src/routes.ts", "orphan")), "");
    assert_eq!(doc_of(member(&map, "src/routes.ts", "admin-users")), "");
    // Cada um leva o dono e a linha do próprio par.
    let home = member(&map, "src/routes.ts", "home");
    assert_eq!(home["line"], 4, "{home}");
    assert_eq!(home["end_line"], 4, "{home}");
    assert_eq!(home["owner"], serde_json::json!(["ROUTES"]), "{home}");
}

const SHAPES: &str = "\
// Objeto puro, sem exportar.
const LOCAL = {
  // Primeiro item.
  first: 1,
};

// Com as const.
export const FROZEN = {
  // Item congelado.
  frozen: 'a',
} as const;

// Com satisfies.
export const CHECKED = {
  // Item conferido.
  checked: 'b',
} satisfies Record<string, string>;

// Com as const e satisfies juntos.
export const BOTH = {
  // Item dos dois.
  both: 'c',
} as const satisfies Record<string, string>;

// Uma variável mutável exportada também vale, como a constante exportada.
export let COUNTER = {
  // Contador.
  count: 0,
};
";

#[test]
fn a_top_level_object_is_read_plain_or_with_as_const_and_satisfies() {
    let (_temp, _dir, map) = scanned("formas", &[("src/shapes.ts", SHAPES)]);

    for (name, doc) in [
        ("first", "Primeiro item."),
        ("frozen", "Item congelado."),
        ("checked", "Item conferido."),
        ("both", "Item dos dois."),
        ("count", "Contador."),
    ] {
        assert_eq!(doc_of(member(&map, "src/shapes.ts", name)), doc, "{name}");
    }
}

const NESTED: &str = "\
export function build(name: string) {
  // Dentro da função: não é um objeto de constantes.
  const local = {
    // Item local.
    inner: 1,
  };
  return {
    // Valor de retorno.
    returned: local.inner,
  };
}

// Argumento de chamada.
export const wrapped = makeRoutes({
  // Item do argumento.
  argument: '/x',
});

// Uma lista de objetos não é um objeto de constantes.
export const LIST = [
  {
    // Item da lista.
    item: 1,
  },
];

// O objeto de dentro de um objeto não é o valor da constante.
export const GROUPS = {
  // Grupo de rotas.
  group: {
    // Rota do grupo.
    deep: '/deep',
  },
};

// A forma curta não é um par, e o método segue sendo método.
export const HANDLERS = {
  handle() {
    return 1;
  },
  short,
};
";

#[test]
fn an_object_inside_a_function_a_call_argument_or_a_return_value_files_no_member() {
    let (_temp, _dir, map) = scanned("aninhado", &[("src/nested.ts", NESTED)]);

    // Só o par que é filho direto do objeto da constante entra.
    assert_eq!(names_of(&map, "src/nested.ts", "enum_member"), vec!["group"], "{:?}", declarations(&map, "src/nested.ts"));
    assert_eq!(names_of(&map, "src/nested.ts", "method"), vec!["handle"]);
    assert_eq!(doc_of(member(&map, "src/nested.ts", "group")), "Grupo de rotas.");
    // Nenhuma outra declaração nasce dos objetos de dentro.
    let all: Vec<&str> = declarations(&map, "src/nested.ts").iter().map(|d| d["name"].as_str().unwrap()).collect();
    for absent in ["inner", "returned", "argument", "item", "deep", "short"] {
        assert!(!all.contains(&absent), "{absent} não é declaração: {all:?}");
    }
}

#[test]
fn the_pairs_of_an_object_constant_are_read_in_tsx_and_javascript_files_too() {
    let source = "\
export const LABELS = {
  // Texto do botão.
  save: 'Salvar',
};
";
    let (_temp, _dir, map) = scanned(
        "tsx-e-js",
        &[
            ("src/labels.tsx", source),
            ("src/labels.js", source),
            ("src/labels.jsx", source),
        ],
    );

    for file in ["src/labels.tsx", "src/labels.js", "src/labels.jsx"] {
        assert_eq!(doc_of(member(&map, file, "save")), "Texto do botão.", "{file}");
        assert_eq!(names_of(&map, file, "enum_member"), vec!["save"], "{file}");
    }
}

const TRAILING: &str = "\
export const STATUS = {
  ACTIVE: 1, // ativo
  INACTIVE: 2, // inativo
  // Encerrado de vez.
  CLOSED: 3, // fechado
};
";

#[test]
fn a_comment_at_the_end_of_the_previous_pair_is_not_the_doc_of_the_next_one() {
    let (_temp, _dir, map) = scanned("comentario-no-fim", &[("src/status.ts", TRAILING)]);

    // O comentário escrito no fim da linha de um par não documenta o par de
    // baixo: só o que está em cima, sozinho na linha, documenta.
    assert_eq!(doc_of(member(&map, "src/status.ts", "ACTIVE")), "");
    assert_eq!(doc_of(member(&map, "src/status.ts", "INACTIVE")), "");
    assert_eq!(doc_of(member(&map, "src/status.ts", "CLOSED")), "Encerrado de vez.");
}
