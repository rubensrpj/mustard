export function Sidebar() {
  return (
    <aside className="row-span-2 col-start-1 bg-sidebar text-sidebar-foreground border-r border-border p-4 flex flex-col gap-2">
      <div className="text-lg font-semibold">Mustard</div>
      <nav className="flex flex-col gap-1 mt-4">
        <a href="/" className="px-3 py-2 rounded-md bg-muted text-foreground font-medium">Home</a>
      </nav>
    </aside>
  );
}
