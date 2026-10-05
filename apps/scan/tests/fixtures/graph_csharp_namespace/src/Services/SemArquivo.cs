using static Loja.Models.Desconto;

namespace Loja.Services;

public class SemArquivo
{
    public decimal Fechar() => Aplicar(10m);
}
