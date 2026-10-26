//! Toda unidade de teste de cada língua do mapa, em cada forma que os
//! frameworks de teste dela usam, sai do mapa marcada como teste
//! (`syntax.test`) e com um nome. O teste que a língua declara (a função com
//! o atributo, o método da classe de teste) leva o nome da declaração; o que
//! é uma chamada (`it('soma', …)`) entra como declaração de teste com o texto
//! do primeiro argumento como nome. O grupo (`describe`, `group`) não é
//! teste, a função de apoio também não, e a forma que a língua só roda no
//! arquivo de teste (o `def test_*` do pytest, o `func TestX` do Go) não é
//! teste fora dele.
//!
//! Um projeto por língua, cada um com todas as formas dela.

#[path = "support/model.rs"]
mod model;

use std::path::Path;

use serde_json::Value;

/// Uma língua: os arquivos do projeto dela, os testes que o mapa marca (o
/// arquivo e o nome), as declarações que ele guarda sem a marca e os nomes
/// que nenhuma declaração do arquivo leva.
struct Language {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
    tests: &'static [(&'static str, &'static str)],
    not_tests: &'static [(&'static str, &'static str)],
    absent: &'static [(&'static str, &'static str)],
}

const RUST: Language = Language {
    name: "rust",
    files: &[
        ("Cargo.toml", "[package]\nname = \"sums\"\nversion = \"0.1.0\"\n"),
        (
            "src/lib.rs",
            "pub fn sum(a: u32, b: u32) -> u32 {\n    a + b\n}\n\n\
             #[cfg(test)]\nmod tests {\n    use super::*;\n\n    \
             fn helper() -> u32 {\n        1\n    }\n\n    \
             #[test]\n    fn plain_check() {\n        assert_eq!(sum(1, helper()), 2);\n    }\n\n    \
             #[tokio::test]\n    async fn tokio_check() {}\n\n    \
             #[tokio::test(flavor = \"multi_thread\")]\n    async fn tokio_check_with_arguments() {}\n\n    \
             #[async_std::test]\n    async fn async_std_check() {}\n\n    \
             #[rstest]\n    #[case(1)]\n    fn rstest_check(#[case] x: u32) {\n        assert_eq!(x, 1);\n    }\n\n    \
             #[test_case(1 ; \"one\")]\n    #[test_case(2 ; \"two\")]\n    fn test_case_check(x: u32) {\n        assert!(x > 0);\n    }\n\n    \
             #[proptest]\n    fn proptest_check(x: u32) {\n        assert!(x >= 0);\n    }\n\n    \
             #[quickcheck]\n    fn quickcheck_check(x: u32) -> bool {\n        x == x\n    }\n\n    \
             #[test]\n    #[should_panic]\n    fn panicking_check() {\n        panic!();\n    }\n}\n",
        ),
        ("tests/api.rs", "#[test]\nfn loose_check_in_a_test_file() {\n    assert_eq!(sums::sum(1, 1), 2);\n}\n"),
    ],
    tests: &[
        ("src/lib.rs", "plain_check"),
        ("src/lib.rs", "tokio_check"),
        ("src/lib.rs", "tokio_check_with_arguments"),
        ("src/lib.rs", "async_std_check"),
        ("src/lib.rs", "rstest_check"),
        ("src/lib.rs", "test_case_check"),
        ("src/lib.rs", "proptest_check"),
        ("src/lib.rs", "quickcheck_check"),
        ("src/lib.rs", "panicking_check"),
        ("tests/api.rs", "loose_check_in_a_test_file"),
    ],
    not_tests: &[("src/lib.rs", "sum"), ("src/lib.rs", "helper")],
    absent: &[],
};

/// O mesmo teste do TypeScript, do TSX e do JavaScript: a família é uma só.
const TYPESCRIPT: Language = Language {
    name: "typescript",
    files: &[
        ("src/sum.ts", "export function sum(a: number, b: number): number {\n  return a + b;\n}\n"),
        (
            "src/sum.test.ts",
            "import { sum } from \"./sum\";\n\n\
             describe(\"sum\", () => {\n  \
             it(\"adds two numbers\", () => {\n    expect(sum(1, 2)).toBe(3);\n  });\n  \
             test('adds zero', async () => {\n    expect(sum(1, 0)).toBe(1);\n  });\n  \
             it.each([[1, 1]])(\"adds %i to itself\", (a, b) => {\n    expect(sum(a, b)).toBe(2);\n  });\n  \
             test.each([[2]])('doubles %i', (a) => {\n    expect(sum(a, a)).toBe(4);\n  });\n  \
             test.each`\n    a\n    ${1}\n  `('adds $a by table', ({ a }) => {\n    expect(sum(a, 0)).toBe(a);\n  });\n  \
             it.only(\"adds only this one\", () => {});\n  \
             test.skip(`skips ${\"this\"} one`, function () {});\n  \
             test(\"takes options\", { timeout: 10 }, () => {});\n\
             });\n",
        ),
        ("src/view.test.tsx", "test(\"renders the total\", () => {\n  const view = <p>3</p>;\n  expect(view).toBeTruthy();\n});\n"),
        ("src/sum.spec.js", "const { sum } = require(\"./sum\");\n\nit(\"adds in javascript\", function () {\n  sum(1, 2);\n});\n"),
    ],
    tests: &[
        ("src/sum.test.ts", "adds two numbers"),
        ("src/sum.test.ts", "adds zero"),
        ("src/sum.test.ts", "adds %i to itself"),
        ("src/sum.test.ts", "doubles %i"),
        ("src/sum.test.ts", "adds $a by table"),
        ("src/sum.test.ts", "adds only this one"),
        ("src/sum.test.ts", "skips ${\"this\"} one"),
        ("src/sum.test.ts", "takes options"),
        ("src/view.test.tsx", "renders the total"),
        ("src/sum.spec.js", "adds in javascript"),
    ],
    not_tests: &[("src/sum.ts", "sum")],
    absent: &[("src/sum.test.ts", "sum")],
};

const PYTHON: Language = Language {
    name: "python",
    files: &[
        ("app/__init__.py", ""),
        ("app/sums.py", "def add(a, b):\n    return a + b\n\n\ndef test_connection():\n    return True\n"),
        (
            "app/cases.py",
            "import unittest\n\n\n\
             class SumCase(unittest.TestCase):\n    \
             def setUp(self):\n        self.base = 1\n\n    \
             def test_unittest_method(self):\n        self.assertEqual(self.base, 1)\n\n    \
             @unittest.skip(\"later\")\n    def testDecoratedUnittest(self):\n        pass\n",
        ),
        (
            "tests/test_sums.py",
            "import pytest\nfrom unittest import TestCase\nfrom app.sums import add\n\n\n\
             def helper_value():\n    return 1\n\n\n\
             def test_adds():\n    assert add(1, 2) == 3\n\n\n\
             @pytest.mark.parametrize(\"a\", [1, 2])\ndef test_parametrized(a):\n    assert add(a, 0) == a\n\n\n\
             async def test_async_adds():\n    assert add(1, 1) == 2\n\n\n\
             class TestSums:\n    \
             def test_in_a_class(self):\n        assert add(0, 0) == 0\n\n    \
             @pytest.mark.slow\n    def test_decorated_in_a_class(self):\n        pass\n\n    \
             def helper(self):\n        return helper_value()\n\n\n\
             class OtherCase(TestCase):\n    \
             def testCamelCase(self):\n        pass\n",
        ),
    ],
    tests: &[
        ("app/cases.py", "test_unittest_method"),
        ("app/cases.py", "testDecoratedUnittest"),
        ("tests/test_sums.py", "test_adds"),
        ("tests/test_sums.py", "test_parametrized"),
        ("tests/test_sums.py", "test_async_adds"),
        ("tests/test_sums.py", "test_in_a_class"),
        ("tests/test_sums.py", "test_decorated_in_a_class"),
        ("tests/test_sums.py", "testCamelCase"),
    ],
    not_tests: &[
        ("app/sums.py", "add"),
        ("app/sums.py", "test_connection"),
        ("app/cases.py", "setUp"),
        ("tests/test_sums.py", "helper_value"),
        ("tests/test_sums.py", "helper"),
    ],
    absent: &[],
};

const CSHARP: Language = Language {
    name: "csharp",
    files: &[
        ("src/Sums.cs", "namespace Sums;\n\npublic static class Calc\n{\n    public static int Add(int a, int b) => a + b;\n}\n"),
        (
            "tests/Sums.Tests/CalcTests.cs",
            "namespace Sums.Tests;\n\npublic class CalcTests\n{\n    \
             [Fact]\n    public void AddsWithFact() { }\n\n    \
             [Theory]\n    [InlineData(1)]\n    public void AddsWithTheory(int a) { }\n\n    \
             [Fact(Skip = \"later\")]\n    public async Task AddsLater() { }\n\n    \
             [Xunit.Fact]\n    public void AddsWithQualifiedFact() { }\n\n    \
             [Test]\n    public void AddsWithNUnit() { }\n\n    \
             [TestCase(1, 2)]\n    public void AddsWithTestCase(int a, int b) { }\n\n    \
             [TestMethod]\n    public void AddsWithMSTest() { }\n\n    \
             [DataTestMethod, DataRow(1)]\n    public void AddsWithDataRow(int a) { }\n\n    \
             [SetUp]\n    public void Prepare() { }\n\n    \
             private int Helper() => 1;\n}\n",
        ),
    ],
    tests: &[
        ("tests/Sums.Tests/CalcTests.cs", "AddsWithFact"),
        ("tests/Sums.Tests/CalcTests.cs", "AddsWithTheory"),
        ("tests/Sums.Tests/CalcTests.cs", "AddsLater"),
        ("tests/Sums.Tests/CalcTests.cs", "AddsWithQualifiedFact"),
        ("tests/Sums.Tests/CalcTests.cs", "AddsWithNUnit"),
        ("tests/Sums.Tests/CalcTests.cs", "AddsWithTestCase"),
        ("tests/Sums.Tests/CalcTests.cs", "AddsWithMSTest"),
        ("tests/Sums.Tests/CalcTests.cs", "AddsWithDataRow"),
    ],
    not_tests: &[("src/Sums.cs", "Add"), ("tests/Sums.Tests/CalcTests.cs", "Prepare"), ("tests/Sums.Tests/CalcTests.cs", "Helper")],
    absent: &[],
};

const GO: Language = Language {
    name: "go",
    files: &[
        ("go.mod", "module example.com/sums\n\ngo 1.22\n"),
        ("sums/sums.go", "package sums\n\nimport \"testing\"\n\nfunc Add(a, b int) int {\n\treturn a + b\n}\n\nfunc TestLookalike(t *testing.T) {}\n"),
        (
            "sums/sums_test.go",
            "package sums\n\nimport \"testing\"\n\n\
             type SumSuite struct{}\n\n\
             func TestAdds(t *testing.T) {\n\tif Add(1, 2) != 3 {\n\t\tt.Fail()\n\t}\n}\n\n\
             func BenchmarkAdds(b *testing.B) {\n\tfor i := 0; i < b.N; i++ {\n\t\tAdd(1, 2)\n\t}\n}\n\n\
             func FuzzAdds(f *testing.F) {\n\tf.Fuzz(func(t *testing.T, a int) {})\n}\n\n\
             func ExampleAdd() {\n\tAdd(1, 2)\n\t// Output:\n}\n\n\
             func (s *SumSuite) TestInASuite() {}\n\n\
             func TestMain(m *testing.M) {\n\tm.Run()\n}\n\n\
             func helperForTests(t *testing.T) {}\n",
        ),
    ],
    tests: &[
        ("sums/sums_test.go", "TestAdds"),
        ("sums/sums_test.go", "BenchmarkAdds"),
        ("sums/sums_test.go", "FuzzAdds"),
        ("sums/sums_test.go", "ExampleAdd"),
        ("sums/sums_test.go", "TestInASuite"),
    ],
    not_tests: &[
        ("sums/sums.go", "Add"),
        ("sums/sums.go", "TestLookalike"),
        ("sums/sums_test.go", "TestMain"),
        ("sums/sums_test.go", "helperForTests"),
    ],
    absent: &[],
};

const PHP: Language = Language {
    name: "php",
    files: &[
        ("app/Sums.php", "<?php\n\nnamespace App;\n\nclass Sums\n{\n    public function testable(): bool\n    {\n        return true;\n    }\n}\n"),
        (
            "tests/SumsTest.php",
            "<?php\n\nnamespace Tests;\n\nuse PHPUnit\\Framework\\Attributes\\Test;\nuse PHPUnit\\Framework\\TestCase;\n\n\
             final class SumsTest extends TestCase\n{\n    \
             public function testAdds(): void\n    {\n    }\n\n    \
             /**\n     * @test\n     */\n    public function adds_by_annotation(): void\n    {\n    }\n\n    \
             #[Test]\n    public function adds_by_attribute(): void\n    {\n    }\n\n    \
             #[\\PHPUnit\\Framework\\Attributes\\Test]\n    public function adds_by_qualified_attribute(): void\n    {\n    }\n\n    \
             protected function setUp(): void\n    {\n    }\n\n    \
             private function helper(): int\n    {\n        return 1;\n    }\n}\n",
        ),
        (
            "tests/Unit/CalcCheck.php",
            "<?php\n\nnamespace Tests\\Unit;\n\nclass CalcCheck extends \\Tests\\TestCase\n{\n    public function testExtendsTheProjectCase(): void\n    {\n    }\n}\n",
        ),
        (
            "tests/Feature/SumsPest.php",
            "<?php\n\ntest('adds with pest', function () {\n    expect(1 + 2)->toBe(3);\n});\n\n\
             it(\"adds with it\", fn () => expect(true)->toBeTrue());\n\n\
             describe('sums', function () {\n    it('adds inside a group', function () {\n    });\n});\n",
        ),
    ],
    tests: &[
        ("tests/SumsTest.php", "testAdds"),
        ("tests/SumsTest.php", "adds_by_annotation"),
        ("tests/SumsTest.php", "adds_by_attribute"),
        ("tests/SumsTest.php", "adds_by_qualified_attribute"),
        ("tests/Unit/CalcCheck.php", "testExtendsTheProjectCase"),
        ("tests/Feature/SumsPest.php", "adds with pest"),
        ("tests/Feature/SumsPest.php", "adds with it"),
        ("tests/Feature/SumsPest.php", "adds inside a group"),
    ],
    not_tests: &[("app/Sums.php", "testable"), ("tests/SumsTest.php", "setUp"), ("tests/SumsTest.php", "helper")],
    absent: &[("tests/Feature/SumsPest.php", "sums")],
};

const DART: Language = Language {
    name: "dart",
    files: &[
        ("pubspec.yaml", "name: sums\n"),
        ("lib/sums.dart", "int add(int a, int b) => a + b;\n"),
        (
            "test/sums_test.dart",
            "import 'package:flutter_test/flutter_test.dart';\nimport 'package:sums/sums.dart';\n\n\
             void main() {\n  group('sums', () {\n    \
             test('adds two numbers', () {\n      expect(add(1, 2), 3);\n    });\n    \
             testWidgets(\"renders the total\", (tester) async {\n      expect(add(1, 1), 2);\n    });\n  \
             });\n}\n",
        ),
    ],
    tests: &[("test/sums_test.dart", "adds two numbers"), ("test/sums_test.dart", "renders the total")],
    not_tests: &[("lib/sums.dart", "add"), ("test/sums_test.dart", "main")],
    absent: &[("test/sums_test.dart", "sums")],
};

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// As declarações de nome `name` do arquivo `path` no mapa `map`.
fn declarations<'a>(map: &'a Value, path: &str, name: &str) -> Vec<&'a Value> {
    let modules = map["modules"].as_array().into_iter().flatten();
    let module = modules.into_iter().find(|m| m["path"] == path);
    let decls = module.and_then(|m| m["declarations"].as_array()).into_iter().flatten();
    decls.filter(|d| d["name"] == name).collect()
}

/// A declaração tem a marca de teste do mapa.
fn marked(decl: &Value) -> bool {
    decl["syntax"]["test"] == Value::Bool(true)
}

/// O que o mapa da língua `language` diz de errado das formas dela: cada
/// teste sem a marca ou sem declaração, cada declaração de fora com a marca
/// e cada nome de grupo que virou declaração.
fn wrong_forms(language: &Language) -> Vec<String> {
    let temp = tempfile::Builder::new().prefix(&format!("scan-test-units-{}-", language.name)).tempdir().unwrap();
    let dir = temp.path();
    for (path, body) in language.files {
        write(dir, path, body);
    }
    let (map, _) = model::scan(dir, &dir.join(".claude"), &[]);
    let mut wrong = Vec::new();
    for (path, name) in language.tests {
        let found = declarations(&map, path, name);
        if !found.iter().any(|decl| marked(decl)) {
            wrong.push(format!("{}: {path} `{name}` não sai marcado como teste ({} declarações com o nome)", language.name, found.len()));
        }
    }
    for (path, name) in language.not_tests {
        let found = declarations(&map, path, name);
        if found.is_empty() || found.iter().any(|decl| marked(decl)) {
            wrong.push(format!("{}: {path} `{name}` devia estar no mapa sem a marca de teste ({found:?})", language.name));
        }
    }
    for (path, name) in language.absent {
        let found = declarations(&map, path, name);
        if !found.is_empty() {
            wrong.push(format!("{}: {path} `{name}` é grupo, e não declaração ({found:?})", language.name));
        }
    }
    wrong
}

/// Cada forma de teste de cada língua sai do mapa marcada como teste e com o
/// nome dela; o grupo, a função de apoio e a forma fora do arquivo em que a
/// língua a roda saem sem a marca.
#[test]
fn every_test_form_is_marked_as_a_test_in_every_language() {
    let wrong: Vec<String> = [RUST, TYPESCRIPT, PYTHON, CSHARP, GO, PHP, DART].iter().flat_map(wrong_forms).collect();
    assert!(wrong.is_empty(), "{} formas erradas:\n{}", wrong.len(), wrong.join("\n"));
}
