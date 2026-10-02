using Demo.Models;

namespace Demo.Services;

public class UserService(string prefix)
{
    public User Load() => new User();
}
