import { useTranslation } from 'react-i18next';
import { useStore } from '@/lib/store';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { PageHeader } from '@/components/page';

// TODO: future preferences (theme, telemetry opt-in) entram aqui.
export function Preferences() {
  const { t } = useTranslation();
  const language = useStore((s) => s.language);
  const setLanguage = useStore((s) => s.setLanguage);

  return (
    <div className="flex flex-col gap-4 w-full">
      <PageHeader
        breadcrumb={["Mustard", t('preferences.title')]}
        title={t('preferences.title')}
        description={t('preferences.description')}
      />
      <Card size="sm">
        <CardHeader>
          <CardTitle className="text-sm font-medium">{t('preferences.language')}</CardTitle>
          <CardDescription className="text-[13px] text-muted-foreground">
            {t('preferences.description')}
          </CardDescription>
        </CardHeader>
        <div className="px-4 pb-4 flex items-center gap-2">
          {(['pt', 'en'] as const).map((lng) => (
            <button
              key={lng}
              onClick={() => setLanguage(lng)}
              className={language === lng
                ? "bg-primary text-primary-foreground px-3 py-1.5 rounded text-sm"
                : "text-muted-foreground hover:text-foreground px-3 py-1.5 rounded text-sm border border-border"}
            >
              {lng === 'pt' ? t('preferences.languagePt') : t('preferences.languageEn')}
            </button>
          ))}
        </div>
      </Card>
    </div>
  );
}
