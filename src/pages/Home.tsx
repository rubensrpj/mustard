import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

export function Home() {
  return (
    <div className="flex flex-col gap-6">
      <Card>
        <CardHeader>
          <CardTitle>Mustard Dashboard — scaffold ready</CardTitle>
          <CardDescription>
            Bootstrap Tauri 2 com React 19, Tailwind v4, shadcn/ui e quatro plugins desktop. Pronto para receber telas reais.
          </CardDescription>
        </CardHeader>
      </Card>
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        {[
          { title: "Pipelines", desc: "Orquestração de execuções de feature, bugfix e review." },
          { title: "Métricas", desc: "Token savings, taxa de retry e tempo por wave." },
          { title: "Knowledge", desc: "Padrões e convenções capturados pelo /scan." },
        ].map((c) => (
          <Card key={c.title}>
            <CardHeader>
              <CardTitle className="text-base">{c.title}</CardTitle>
              <CardDescription>{c.desc}</CardDescription>
            </CardHeader>
          </Card>
        ))}
      </div>
    </div>
  );
}
