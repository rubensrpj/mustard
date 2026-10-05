namespace Loja.Models;

public class Produto
{
    public string Codigo { get; set; } = "";
}

public static class Desconto
{
    public static decimal Aplicar(decimal valor) => valor * 0.9m;
}
