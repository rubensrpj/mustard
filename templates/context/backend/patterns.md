# Backend Patterns (.NET)

> Project-specific patterns for .NET backend implementation.

## Module Structure

```
Modules/{Entity}/
+-- Endpoints/
|   +-- Create{Entity}Endpoint.cs
|   +-- Get{Entity}Endpoint.cs
|   +-- List{Entities}Endpoint.cs
|   +-- Update{Entity}Endpoint.cs
|   +-- Delete{Entity}Endpoint.cs
+-- Services/
|   +-- I{Entity}Service.cs
|   +-- {Entity}Service.cs
+-- Mappers/
    +-- {Entity}Mapper.cs
```

## Layer Architecture

```
+-------------------------------------------------------------+
|                    BACKEND LAYERS                           |
+-------------------------------------------------------------+
|  GraphQL Resolver  -------->  DataBaseContext (direct)      |
|                              - HotChocolate pattern         |
|                              - IQueryable for filtering     |
+-------------------------------------------------------------+
|  REST Endpoint     -------->  Service                       |
|                              - Business logic               |
+-------------------------------------------------------------+
|  Service           -------->  Repository + UnitOfWork       |
|                              - NEVER accesses DbContext     |
|                              - Uses Repository for queries  |
|                              - Uses UnitOfWork for txns     |
+-------------------------------------------------------------+
|  Repository        -------->  DataBaseContext               |
|                              - Only place that accesses DB  |
|                              - Encapsulated queries         |
+-------------------------------------------------------------+
```

---

## Repository ISP (Interface Segregation)

### Base Interfaces

```
IRepository<T>
    +-- IReadRepository<T>       -> Read only
    +-- IWriteRepository<T>      -> Write with auto SaveChanges
    +-- IUnitOfWorkRepository<T> -> Write without SaveChanges (for transactions)
```

### IReadRepository<T>

```csharp
public interface IReadRepository<T> where T : class
{
    Task<IList<T>> GetAllAsync();
    Task<IList<T>> GetByConditionAsync(Expression<Func<T, bool>> predicate);
    Task<T?> GetByIdAsync(uint id);
    Task<bool> ExistsAsync(Expression<Func<T, bool>> predicate);
    IQueryable<T> Query();
}
```

**Usage:** Endpoints and services that only read data.

### IWriteRepository<T>

```csharp
public interface IWriteRepository<T> where T : class
{
    Task<T> CreateAsync(T entity);
    Task<T> UpdateAsync(T entity);
    Task DeleteAsync(T entity);
    Task SaveChangesAsync();
}
```

**Usage:** Simple CRUD operations where each operation is atomic.

### IUnitOfWorkRepository<T>

```csharp
public interface IUnitOfWorkRepository<T> where T : class
{
    void Add(T entity);
    void Modify(T entity);
    void Remove(T entity);
    // NO SaveChanges - controlled by IUnitOfWork
}
```

**Usage:** Complex transactions involving multiple entities.

### DI Registration

```csharp
// In Module, register segregated interfaces as aliases
services.AddScoped<IContractRepository, ContractRepository>();
services.AddScoped<IReadRepository<Contract>>(sp =>
    sp.GetRequiredService<IContractRepository>());
services.AddScoped<IWriteRepository<Contract>>(sp =>
    sp.GetRequiredService<IContractRepository>());
```

---

## SOLID Rules (L7-L9)

> For .NET projects. See full details in [enforcement.md](../../core/enforcement.md#l7-l9---solid-architecture-net).

| Rule | Summary |
|------|---------|
| L7 | Service does NOT access DbContext (uses Repository) |
| L8 | Service only injects its OWN Repository |
| L9 | Prefer segregated interfaces (ISP) |

### SOLID Checklist

```
[ ] Segregated interfaces created (L9)
    [ ] I{Entity}QueryService
    [ ] I{Entity}*Service (as needed)
[ ] Complete interface inherits from segregated ones
[ ] Module registration includes aliases
[ ] Service does not access DbContext (L7)
[ ] Service only injects its own Repository (L8)
[ ] Endpoints use specific interface when possible (L9)
```
