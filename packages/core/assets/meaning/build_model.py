"""Gera o modelo de sentido embutido no Mustard.

Parte do modelo estático potion-multilingual-128M (MIT, destilado do
BAAI/bge-m3), poda o vocabulário para as palavras gerais das línguas e para
pedaços de nomes de código genéricos, e grava os vetores em int8. Nenhum
texto de projeto entra na poda.

Uso, com o modelo já baixado do Hugging Face:

    python build_model.py <pasta de saída> [língua ...]

Sem línguas, usa as do Mustard em `normalize/languages/`. O modelo final não
pode passar de 30 MB: passando, o script refaz só com português e inglês.
"""
import json
import os
import sys

from model2vec import StaticModel
from model2vec.quantization import DType, quantize_embeddings
from tokenizers import Tokenizer
from wordfreq import top_n_list

SOURCE = "minishlab/potion-multilingual-128M"
MAX_BYTES = 30_000_000
ALL_LANGUAGES = ["ar", "da", "de", "el", "en", "es", "fi", "fr", "hu", "it", "nl", "nb", "pt", "ro", "ru", "sv", "ta", "tr"]
WORDS_PER_LANGUAGE = 30_000
FIRST_LANGUAGES = ["pt", "en"]
FIRST_WORDS = 100_000

# Pedaços de nomes de código que valem em qualquer projeto: verbos e
# substantivos de programa, siglas e abreviações comuns.
CODE_WORDS = """
get set add remove delete update create find search list read write open close load save parse build run start stop
init config configure path file dir folder name id key value type kind index count size length map filter sort merge
split join hash cache buffer stream queue stack tree node graph edge token span range line column row table field
record item entry event handler hook callback request response error result status state context session user account
order price total date time timestamp check validate verify assert test mock stub fixture render format print log
warn debug info trace fail success retry timeout wait sleep spawn thread task job worker pool lock mutex channel
send receive emit listen subscribe publish notify dispatch route controller service repository factory builder
adapter wrapper proxy client server host port url uri api rest http https json xml yaml toml csv sql db database
query select insert commit rollback transaction migrate schema model view template component page screen button
form input output param argument option flag setting default env var const let mut static public private protected
abstract interface class struct enum trait impl fn func function method module package import export require use
using namespace async await promise future stream iterator generator visitor observer decorator singleton
ctx cfg dto tmp src dst ptr len idx num str int bool fmt msg req res err ok none some null nil undefined
encode decode encrypt decrypt compress serialize deserialize marshal unmarshal convert transform normalize
sanitize escape unescape trim strip replace match regex pattern glob prefix suffix header footer body title
label text message note comment doc summary description version release tag branch merge pull push clone fetch
diff patch stage stash blame history author owner member group role permission policy rule limit quota
metric counter gauge histogram trace span sample batch chunk block page offset cursor scan walk visit
resolve register unregister bind unbind attach detach mount unmount enable disable toggle reset clear flush
sync async parallel concurrent atomic volatile shared unique weak strong lazy eager cached pending done ready
""".split()


def general_words(languages, count):
    words = []
    for language in languages:
        try:
            words += top_n_list(language, count)
        except LookupError:
            print("sem lista para", language, file=sys.stderr)
    return words


def prune(model, words):
    tokenizer = model.tokenizer
    keep = set()
    for start in range(0, len(words), 2000):
        for encoding in tokenizer.encode_batch(words[start:start + 2000], add_special_tokens=False):
            keep.update(encoding.ids)
    spec = json.loads(tokenizer.to_str())
    vocab = spec["model"]["vocab"]
    for index, (piece, _) in enumerate(vocab):
        if len(piece) == 1 or piece.startswith("<"):
            keep.add(index)
    unk = spec["model"].get("unk_id")
    if unk is not None:
        keep.add(unk)
    ids = sorted(keep)
    remap = {old: new for new, old in enumerate(ids)}
    spec["model"]["vocab"] = [vocab[i] for i in ids]
    if unk is not None:
        spec["model"]["unk_id"] = remap[unk]
    # O token adicionado fora do vocabulário (a máscara) não tem vetor: sai.
    spec["added_tokens"] = [added for added in spec.get("added_tokens", []) if added["id"] in remap]
    for added in spec["added_tokens"]:
        added["id"] = remap[added["id"]]
    return Tokenizer.from_str(json.dumps(spec)), ids


def build(out, languages):
    model = StaticModel.from_pretrained(SOURCE)
    words = CODE_WORDS + general_words(FIRST_LANGUAGES, FIRST_WORDS) + general_words(
        [language for language in languages if language not in FIRST_LANGUAGES], WORDS_PER_LANGUAGE
    )
    tokenizer, ids = prune(model, words)
    small = StaticModel(quantize_embeddings(model.embedding[ids], DType.Int8), tokenizer, model.config, normalize=True)
    small.save_pretrained(out)
    size = sum(os.path.getsize(os.path.join(out, name)) for name in ("model.safetensors", "tokenizer.json", "config.json"))
    print("línguas", languages, "peças", len(ids), "bytes", size)
    return size


if __name__ == "__main__":
    target = sys.argv[1]
    chosen = sys.argv[2:] or ALL_LANGUAGES
    if build(target, chosen) > MAX_BYTES:
        print("passou de 30 MB: só português e inglês", file=sys.stderr)
        build(target, FIRST_LANGUAGES)
