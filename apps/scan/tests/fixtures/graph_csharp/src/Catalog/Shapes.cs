namespace Demo.Catalog;

public interface IShape
{
    int Area();
}

public record Point(int X, int Y);

public struct Size
{
    public int Width;
}

public enum Status
{
    Active,
    Inactive,
}
