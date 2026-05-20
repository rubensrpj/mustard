import { useState, useMemo } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import { useStore } from '@/lib/store';
import { discoverProjects } from '@/api/discovery';
import { readEnv, writeEnv } from '@/api/env';
import { ENV_CATALOG, type EnvKey } from '@/data/env-catalog';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { CollapsibleGroup } from '@/components/page';

function omitKey(obj: Record<string, string>, key: string): Record<string, string> {
  const next = { ...obj };
  delete next[key];
  return next;
}

/**
 * One environment-variable row. The human-readable `label` is the primary
 * heading; the raw env var name sits below as a monospace subtitle so it is
 * still discoverable without dominating the form.
 */
function EnvField({
  envKey: k,
  value,
  pending,
  onChange,
}: {
  envKey: EnvKey;
  value: string;
  pending: boolean;
  onChange: (key: string, value: string) => void;
}) {
  const inputId = `env-${k.key}`;
  return (
    <div className="px-4 pb-3 pt-1 flex flex-col gap-1">
      <div className="flex items-baseline gap-2 flex-wrap">
        <label htmlFor={inputId} className="text-[13px] font-medium text-foreground">
          {k.label}
        </label>
        {pending && (
          <Badge variant="outline" className="text-[10px] border-amber-500/40 text-amber-300">
            alteração pendente
          </Badge>
        )}
      </div>
      <code className="font-mono text-[11px] text-muted-foreground/70">{k.key}</code>
      <p className="text-[12.5px] text-muted-foreground leading-relaxed">{k.desc}</p>
      {k.options.length === 0 ? (
        <input
          id={inputId}
          className="bg-card border border-border rounded-md text-sm px-2 py-1 focus:border-primary outline-none w-full transition-colors"
          value={value}
          onChange={(e) => onChange(k.key, e.target.value)}
          placeholder={k.default || 'vazio'}
        />
      ) : (
        <select
          id={inputId}
          className="bg-card border border-border rounded-md text-sm px-2 py-1 focus:border-primary outline-none w-full transition-colors"
          value={value}
          onChange={(e) => onChange(k.key, e.target.value)}
        >
          {k.options.map((opt) => (
            <option key={opt} value={opt}>
              {opt === '' ? '(vazio)' : opt}
              {opt === k.default ? ' — padrão' : ''}
            </option>
          ))}
        </select>
      )}
      {k.valueDocs[value] && (
        <p className="text-[12px] text-muted-foreground/70 mt-0.5">
          {k.valueDocs[value]}
        </p>
      )}
    </div>
  );
}

export function Settings() {
  const { t } = useTranslation();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const selectedProjectId = useStore((s) => s.selectedProjectId);

  const { data: projects } = useQuery({
    queryKey: ['discover', projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  const selectedProject = useMemo(
    () => projects?.find((p) => p.id === selectedProjectId) ?? null,
    [projects, selectedProjectId],
  );

  const queryClient = useQueryClient();
  const [pendingEnv, setPendingEnv] = useState<Record<string, string>>({});

  const { data: envFromDisk } = useQuery({
    queryKey: ['env', selectedProject?.path],
    queryFn: () => readEnv(selectedProject!.path),
    enabled: !!selectedProject,
    staleTime: 60_000,
  });

  const effectiveEnv = useMemo(
    () => ({ ...envFromDisk, ...pendingEnv }),
    [envFromDisk, pendingEnv],
  );

  const hasPending = useMemo(() => Object.keys(pendingEnv).length > 0, [pendingEnv]);

  const saveMutation = useMutation({
    mutationFn: (env: Record<string, string>) => writeEnv(selectedProject!.path, env),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['env', selectedProject?.path] });
      setPendingEnv({});
      toast.success('Salvo');
    },
    onError: (e: Error) => toast.error('Erro: ' + e.message),
  });

  function onSelectChange(key: string, value: string) {
    setPendingEnv((prev) =>
      value === (envFromDisk ?? {})[key]
        ? omitKey(prev, key)
        : { ...prev, [key]: value },
    );
  }

  function onSave() {
    saveMutation.mutate(effectiveEnv);
  }

  function onDiscard() {
    setPendingEnv({});
  }

  return (
    <div className="flex flex-col gap-4 w-full">
      <div className="flex flex-col gap-1">
        <nav className="text-[12px] text-muted-foreground">
          Mustard <span className="opacity-50">/</span>{" "}
          <span className="text-foreground">{t('settings.title')}</span>
        </nav>
        <h1 className="text-xl font-medium tracking-tight">{t('settings.title')}</h1>
        <p className="text-[13px] text-muted-foreground leading-relaxed">
          {t('settings.envDescriptionBefore')}
          <code className="font-mono">.claude/settings.json#env</code>
          {t('settings.envDescriptionAfter')}
        </p>
      </div>
      {/* Environment section */}
      {!selectedProject ? (
        <p className="text-[13px] text-muted-foreground">
          {t('settings.envSelectProject')}
        </p>
      ) : (
        <>
          <div>
            <h2 className="text-sm font-medium">{t('settings.envTitle')} — {selectedProject.name}</h2>
            <p className="text-[13px] text-muted-foreground">
              {t('settings.envHelpTextBefore')}<code className="font-mono">MUSTARD_*</code>{t('settings.envHelpTextSep')}<code className="font-mono">OTEL_*</code>{t('settings.envHelpTextSep')}<code className="font-mono">CLAUDE_CODE_*</code>{t('settings.envHelpTextAfter')}<code className="font-mono">.claude/settings.json#env</code>{t('settings.envHelpTextEnd')}
            </p>
          </div>
          {ENV_CATALOG.map((group) => {
            const basicKeys = group.keys.filter((k) => !k.advanced);
            const advancedKeys = group.keys.filter((k) => k.advanced);
            return (
              <Card key={group.group} size="sm">
                <CardHeader>
                  <CardTitle className="text-sm font-medium">{group.group}</CardTitle>
                  <CardDescription className="text-[13px] text-muted-foreground">
                    {group.desc}
                  </CardDescription>
                </CardHeader>
                {basicKeys.map((k) => (
                  <EnvField
                    key={k.key}
                    envKey={k}
                    value={effectiveEnv[k.key] ?? k.default}
                    pending={k.key in pendingEnv}
                    onChange={onSelectChange}
                  />
                ))}
                {advancedKeys.length > 0 && (
                  <div className="px-4 pb-3">
                    <CollapsibleGroup
                      label="Avançado"
                      count={advancedKeys.length}
                      hint="Knobs de baixo nível (porta, protocolo, transporte). A maioria dos usuários nunca precisa mexer."
                    >
                      <div className="flex flex-col -mx-4 mt-1">
                        {advancedKeys.map((k) => (
                          <EnvField
                            key={k.key}
                            envKey={k}
                            value={effectiveEnv[k.key] ?? k.default}
                            pending={k.key in pendingEnv}
                            onChange={onSelectChange}
                          />
                        ))}
                      </div>
                    </CollapsibleGroup>
                  </div>
                )}
              </Card>
            );
          })}
          <div className="flex items-center gap-2 pt-2 border-t border-border">
            <button
              disabled={!hasPending}
              onClick={onSave}
              className="bg-primary text-primary-foreground px-3 py-1.5 rounded text-sm disabled:opacity-40"
            >
              {t('settings.saveChanges')}
            </button>
            <button
              disabled={!hasPending}
              onClick={onDiscard}
              className="text-muted-foreground hover:text-foreground px-3 py-1.5 rounded text-sm border border-border disabled:opacity-40"
            >
              {t('settings.discardChanges')}
            </button>
            <span className="text-[13px] text-muted-foreground ml-auto">
              {Object.keys(pendingEnv).length} {t('settings.envPending')}
            </span>
          </div>
        </>
      )}
    </div>
  );
}
