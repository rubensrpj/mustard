export function Topbar({ title = "Mustard Dashboard" }: { title?: string }) {
  const toggleTheme = () => document.documentElement.classList.toggle("dark");
  return (
    <header className="row-start-1 col-start-2 h-14 sticky top-0 bg-background border-b border-border flex items-center justify-between px-4">
      <h1 className="text-base font-semibold">{title}</h1>
      <button
        type="button"
        onClick={toggleTheme}
        className="px-3 py-1.5 rounded-md border border-border text-sm hover:bg-muted"
      >
        Alternar tema
      </button>
    </header>
  );
}
