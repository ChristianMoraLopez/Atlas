import { useEffect, useMemo, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { BadgeCheck, Check, ClipboardCheck, ExternalLink, FolderOpen, RotateCcw, ShieldCheck, UserRound, X } from 'lucide-react';
import AtlasLoader from './AtlasLoader';
import { api } from './lib';
import { useI18n, useT } from './i18n';
import type { AppRole, InstallerPhase, InstallerSnapshot } from './types';

const steps: [InstallerPhase, string][] = [
  ['checking_requirements', 'Comprobando requisitos'], ['waiting_sign_in', 'Esperando inicio de sesión'],
  ['finding_environment', 'Actualizando instalación anterior'], ['finding_connections', 'Conectando Microsoft 365'],
  ['importing_solution', 'Importando solución'], ['activating_flow', 'Actualizando flujo anterior'],
  ['verifying_file', 'Verificando archivo'], ['completed', 'Instalación completada'],
  ['blocked_by_policy', 'Bloqueado por política corporativa'],
];
const visibleSteps = steps.filter(([key]) => !['finding_environment', 'activating_flow', 'blocked_by_policy'].includes(key));
const diagnostics: Record<string, string> = {
  portal_required: 'El portal oficial realiza la instalación. Atlas no puede leer tu sesión, detectar permisos remotos ni confirmar conexiones automáticamente.',
  user_reported_not_remotely_verified: 'Paso anterior confirmado por ti; Atlas no ha consultado el tenant. Continúa en el mismo entorno y con tu cuenta corporativa.',
  file_verified: 'Llegó un archivo de esta instalación y cumple atlas-evidence.schema.json. Esto verifica la entrega del puente; no certifica la integridad de todos tus datos de Microsoft 365.',
  previous_installation_detected: 'Atlas encontró evidencia de una instalación anterior. Importa el ZIP que preparó este asistente: actualiza tu propia solución AtlasBridge de esta instalación; los archivos antiguos no se borrarán.',
  collector_file_pending: 'La captura inmediata de Teams ya responde, pero todavía falta el paquete del recolector programado. Espera su próximo ciclo; Atlas necesita comprobar también calendario y correo.',
  collector_sources_incomplete: 'El recolector actualizado creó un paquete, pero al menos una conexión de calendario, correo o Teams falló. Abre el historial de “Atlas - Export evidence to OneDrive”, corrige la conexión indicada y vuelve a comprobar.',
  waiting_for_sync: 'Todavía no hay un paquete Atlas válido disponible localmente. El flujo se ejecuta cada cinco minutos. Comprueba su historial y marca AtlasBridge como Siempre mantener en este dispositivo en OneDrive; Atlas volverá a comprobar cada 15 segundos.',
  evidence_not_downloaded: 'OneDrive muestra el archivo, pero todavía no está disponible localmente. Haz clic derecho sobre inbox y elige Siempre mantener en este dispositivo.',
  evidence_too_old: 'El archivo de esta instalación tiene más de 48 horas. Ejecuta el flujo para generar una comprobación reciente.',
  evidence_from_future: 'La fecha del archivo está adelantada respecto al reloj del equipo. Comprueba fecha, hora y zona horaria de Windows y vuelve a ejecutar el flujo.',
  invalid_evidence: 'Hay un archivo de esta instalación incompleto o incompatible con el contrato. Espera a que termine la sincronización y revisa el historial del flujo si persiste.',
  inbox_unavailable: 'No se puede leer la carpeta. Comprueba que OneDrive esté disponible y que la carpeta no se haya movido.',
  environment_maker_required: 'Microsoft indica que falta Environment Maker en el entorno. Atlas no puede conceder ese rol. Si no tienes otro entorno ya autorizado, esta instalación no es posible bajo las restricciones actuales.',
  dataverse_required: 'El entorno no tiene una base de datos Dataverse utilizable. Las soluciones la requieren. No se creará una base ni se solicitarán permisos administrativos.',
  import_privilege_required: 'El portal indica que faltan privilegios de solución, flujo o referencia de conexión en Dataverse. El nombre prv… del mensaje de Microsoft identifica el privilegio exacto. Environment Maker por sí solo no garantiza todos los permisos de importación.',
  missing_prvImportCustomizations: 'Microsoft indica que falta prvImportCustomizations (importar personalizaciones) en este entorno Dataverse. Atlas no puede conceder ese privilegio.',
  missing_prvCreateWorkflow: 'Microsoft indica que falta prvCreateWorkflow (crear procesos/flujos) en este entorno Dataverse.',
  missing_prvCreateConnectionReference: 'Microsoft indica que falta prvCreateConnectionReference (crear referencias de conexión) en este entorno Dataverse.',
  missing_prvReadSolution: 'Microsoft indica que falta prvReadSolution (leer soluciones) en este entorno Dataverse.',
  missing_prvCreateSolution: 'Microsoft indica que falta prvCreateSolution (crear soluciones) en este entorno Dataverse.',
  connector_policy: 'Una política DLP bloquea el conector o la combinación de Outlook, Teams y OneDrive. Atlas no puede modificarla. No se intentará sustituir el conector para sortear la política.',
  outlook_access: 'La conexión Office 365 Outlook de tu cuenta no está disponible o no está autorizada. Solo puedes usar una conexión propia permitida por la empresa.',
  teams_access: 'La conexión Microsoft Teams de tu cuenta no está disponible o no está autorizada. La solución requiere acceso legítimo a este conector.',
  onedrive_access: 'La conexión OneDrive for Business de tu cuenta no está disponible o no está autorizada. Debe corresponder a la cuenta que sincroniza la carpeta elegida.',
  admin_consent_required: 'Microsoft requiere consentimiento que esta cuenta no puede conceder. No hay una ruta automática autorizada para continuar en ese entorno.',
  conditional_access: 'Microsoft devolvió AADSTS53003: una política de acceso condicional bloquea el inicio de sesión. Atlas no puede alterar ese requisito.',
  unmanaged_blocked: 'El entorno bloquea personalizaciones no administradas. Este paquete es una solución no administrada; no se cambiará de tipo para sortear la política.',
  license_required: 'Microsoft indica que falta una licencia aplicable. Los conectores son estándar, pero los derechos asignados, el entorno y sus políticas también deben permitir el uso.',
  throttled: 'Microsoft limita temporalmente las solicitudes. Espera antes de reintentar. Comprueba si la importación ya terminó para evitar repetirla.',
  timeout: 'La operación agotó el tiempo de espera. Puede haber terminado en el servidor: revisa Soluciones y el historial antes de volver a importar.',
  unclassified_portal_error: 'No se pudo identificar la causa con certeza. Un 403 por sí solo no demuestra qué permiso falta. Revisa el detalle oficial; puedes introducir solo el código de error, sin credenciales.',
};
const qaDiagnostics: Record<string, string> = {
  file_verified: 'Tu flujo QA respondió la solicitud de prueba de esta instalación: Outlook, la carpeta de solicitudes y OneDrive funcionan.',
  waiting_for_sync: 'Atlas pidió a tu flujo QA un mensaje de prueba. El flujo revisa las solicitudes cada cinco minutos y OneDrive trae la respuesta; Atlas vuelve a comprobar cada 15 segundos.',
  qa_mail_query_failed: 'Tu flujo QA respondió, pero la búsqueda en Outlook falló. Abre el historial de “Atlas QA - Export mailbox evidence”, revisa la conexión de Office 365 Outlook y vuelve a comprobar.',
  connector_policy: 'Una política DLP bloquea el conector o la combinación de Outlook y OneDrive. Atlas no puede modificarla.',
};

function installationKey(id: string): string {
  return id.toLowerCase().replace(/[^a-z0-9_]/g, '').slice(0, 8);
}

export default function ConnectorInstaller({ role, defaultOwner, onClose, onChangeRole, onUseFolder }: { role: AppRole; defaultOwner?: string; onClose?: () => void; onChangeRole?: () => void; onUseFolder: (folder: string) => Promise<void> }) {
  const t = useT();
  const { lang, setLang } = useI18n();
  const qa = role === 'manager';
  const switchLanguage = async () => {
    const next = lang === 'es' ? 'en' : 'es';
    setLang(next);
    try { await api.saveLanguage(next); } catch { /* the choice still applies to this session */ }
  };
  const [snapshot, setSnapshot] = useState<InstallerSnapshot>();
  const [root, setRoot] = useState('');
  const [owner, setOwner] = useState(defaultOwner ?? '');
  const [ownConnections, setOwnConnections] = useState(false);
  const [syncAccount, setSyncAccount] = useState(false);
  const [report, setReport] = useState('');
  const [busy, setBusy] = useState(false);
  const [busyLabel, setBusyLabel] = useState('Preparando el conector');
  const [error, setError] = useState('');
  const [resetConfirm, setResetConfirm] = useState(false);
  const busyRef = useRef(false);
  const mounted = useRef(true);
  const session = snapshot?.session;
  const phase = session?.phase ?? 'checking_requirements';
  const index = steps.findIndex(([key]) => key === phase);
  const visibleIndex = Math.max(0, visibleSteps.findIndex(([key]) => key === phase));
  const diagnostic = session?.diagnostic ?? 'portal_required';
  const diagnosticText = (qa ? qaDiagnostics[diagnostic] : undefined) ?? diagnostics[diagnostic] ?? 'Revisa el diagnóstico del portal.';
  const connectors = t(qa ? 'Office 365 Outlook y OneDrive for Business' : 'Office 365 Outlook, Microsoft Teams y OneDrive for Business');
  const solutionPreview = useMemo(() => {
    const name = owner.trim().replace(/\s+/g, ' ');
    const key = installationKey(session?.installationId ?? '');
    return `${qa ? 'Atlas QA' : 'Atlas Tracker'} - ${name || t('Tu nombre')} - ${key}`;
  }, [owner, session?.installationId, qa, t]);
  useEffect(() => {
    mounted.current = true;
    api.installer.status(role).then(s => {
      if (!mounted.current) return;
      setSnapshot(s); setRoot(s.oneDriveRoot ?? '');
      if (s.session.ownerName) setOwner(s.session.ownerName);
    }).catch(() => setError(t('No se pudo cargar el asistente. Cierra y vuelve a abrirlo.')));
    return () => { mounted.current = false; };
  }, [role]);
  const run = async (operation: () => Promise<void>, label = 'Procesando la configuración') => {
    if (busyRef.current) return;
    busyRef.current = true; setBusyLabel(label); setBusy(true); setError('');
    try { await operation(); }
    catch (failure) { if (mounted.current) setError(t(localError(failure))); }
    finally { busyRef.current = false; if (mounted.current) setBusy(false); }
  };
  const actionLabels: Record<string, string> = {
    prepare: qa ? 'Preparando tu solución Atlas QA' : 'Preparando tu solución Atlas Tracker',
    confirm_sign_in: 'Guardando la confirmación de Microsoft',
    confirm_environment: 'Actualizando la configuración anterior',
    confirm_connections: 'Preparando la importación del conector',
    confirm_import: 'Confirmando la solución importada',
    confirm_active: 'Iniciando la comprobación del flujo',
    verify: qa ? 'Esperando la respuesta de tu flujo QA' : 'Comprobando calendario, correo y Teams',
    report: 'Interpretando el diagnóstico de Microsoft',
    retry: 'Reanudando la configuración',
    reset: 'Preparando una configuración nueva',
  };
  const act = (action: Record<string, unknown>, quiet = false) => {
    const operation = async () => {
      const next = await api.installer.action(role, action);
      if (mounted.current) setSnapshot(next);
      if (action.kind === 'prepare') await api.installer.openPortal(role);
    };
    if (quiet) {
      // Background polling never covers the screen with the loader.
      if (busyRef.current) return Promise.resolve();
      return operation().catch(failure => { if (mounted.current) setError(t(localError(failure))); });
    }
    return run(operation, t(actionLabels[String(action.kind)] ?? 'Procesando la configuración'));
  };
  useEffect(() => {
    if (phase !== 'verifying_file') return;
    const poll = () => { void act({ kind: 'verify' }, true); };
    poll(); const timer = window.setInterval(poll, 15_000);
    return () => window.clearInterval(timer);
    // Only start/stop on a phase change; the ref prevents overlapping reads.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phase]);
  const portal = () => run(async () => { await api.installer.openPortal(role); }, 'Abriendo el portal de Microsoft');
  const chooseRoot = () => run(async () => {
    const folder = await open({ directory: true, multiple: false, title: t('Selecciona la raíz de tu OneDrive corporativo (no inbox)') });
    if (typeof folder === 'string') { setRoot(folder); setSyncAccount(false); }
  }, 'Abriendo tu OneDrive corporativo');
  const ownerValid = owner.trim().length >= 2;
  const bridgeFolder = qa ? '\\AtlasBridge\\qa' : '\\AtlasBridge\\inbox';

  return <main className="mx-auto min-h-screen max-w-6xl px-10 py-8" aria-busy={busy || !snapshot}>
    <header className="flex items-center justify-between">
      <span className="chip bg-mint text-pine"><ShieldCheck className="h-4 w-4" />{t('Sin instalaciones de sistema')}</span>
      <span className="flex items-center gap-2">
        {onChangeRole && <button className="btn-secondary" onClick={onChangeRole} disabled={busy}>{t('Cambiar rol')}</button>}
        <button title={t('Switch language')} className="rounded-xl border border-ink/10 bg-white px-2.5 py-2.5 text-[11px] font-extrabold tracking-wide hover:bg-mint" onClick={() => void switchLanguage()}>{lang === 'es' ? 'ES' : 'EN'}</button>
        {onClose && <button className="btn-secondary" onClick={onClose} disabled={busy}><X className="h-4 w-4" />{t('Cerrar y conservar progreso')}</button>}
      </span>
    </header>
    <p className="mt-7 text-xs font-bold uppercase tracking-[.18em] text-pine">{qa ? t('Rol QA · Manager') : t('Rol Tracker · CS')}</p>
    <h1 className="mt-2 font-display text-4xl">{qa ? t('Instala tu conector QA') : t('Instala tu conector del tracker')}</h1>
    <p className="mt-3 max-w-4xl text-sm leading-6 text-ink/65">{qa
      ? t('Atlas crea una solución de Power Automate solo para ti con dos flujos: uno exporta los correos de QA que Atlas le pide (historial completo y sincronización dos veces al día) y otro avisa en cuanto un analista auditado te escribe. Usa solo Outlook y OneDrive.')
      : t('Atlas crea una solución de Power Automate solo para ti: exporta tu calendario, correo y Teams cada cinco minutos a tu OneDrive para que Atlas llene el tracker.')}</p>
    <div className="mt-7 grid grid-cols-[280px_1fr] gap-6">
      <aside className="card h-fit p-5">
        <div className="h-1.5 overflow-hidden rounded-full bg-ink/10"><div className="h-full rounded-full bg-pine transition-all" style={{ width: `${((phase === 'completed' ? visibleSteps.length : visibleIndex) / visibleSteps.length) * 100}%` }} /></div>
        <ol className="mt-5 space-y-3" aria-label={t('Etapas de instalación')}>{visibleSteps.map(([key, label], i) => {
          const done = phase === 'completed' || i < visibleIndex;
          const current = key === phase;
          return <li key={key} aria-current={current ? 'step' : undefined} className={`flex items-center gap-3 text-sm ${current ? 'font-bold text-pine' : done ? 'text-ink/70' : 'text-ink/40'}`}>
            <span className={`grid h-6 w-6 shrink-0 place-items-center rounded-full text-[11px] ${done ? 'bg-pine text-white' : current ? 'border-2 border-pine' : 'border border-ink/15'}`}>{done ? <Check className="h-3.5 w-3.5" /> : i + 1}</span>{t(label)}
          </li>;
        })}</ol>
        {session?.solutionDisplayName && <div className="mt-5 rounded-xl bg-mint/50 p-3 text-[11px] leading-5 text-pine"><b className="block">{t('Tu solución')}</b><span className="break-all">{session.solutionDisplayName}</span></div>}
      </aside>
      <section className="card min-w-0 p-7">
        <div role="status" aria-live="polite"><p className="text-xs uppercase tracking-wider text-pine">{index >= 1 && index <= 5 ? t('Esperando confirmación en el portal oficial') : t('Comprobación local de Atlas')}</p><h2 className="mt-2 font-display text-2xl">{t(steps[index][1])}</h2><p className="mt-3 text-sm leading-6 text-ink/65">{t(diagnosticText)}</p></div>
        {phase === 'checking_requirements' && <div className="mt-5 space-y-5">
          <label className="block"><span className="label">{t('Tu nombre (así se llamará tu solución)')}</span>
            <div className="relative"><UserRound className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/35" /><input className="field pl-9" value={owner} maxLength={80} onChange={e => setOwner(e.target.value)} placeholder="Christian Mora" /></div>
            <span className="mt-2 flex items-center gap-2 text-xs text-ink/55"><BadgeCheck className="h-4 w-4 text-pine" />{t('En Power Automate verás:')} <b className="font-mono text-[11px] text-ink/75">{solutionPreview}</b></span>
          </label>
          <p className="text-sm">{t('Necesitas un entorno con Dataverse, Environment Maker o permisos equivalentes para crear el flujo, privilegios de importación y conexiones propias de {connectors}. Estos requisitos se comprueban en Microsoft; Atlas no los da por aprobados.', { connectors })}</p>
          <p className="text-xs text-ink/55">PAC {snapshot?.pacDetected ? t('detectado, pero no se ejecutará') : t('no detectado; no hace falta')}. {t('El asistente usa el navegador y el paquete incorporado. No guarda tokens ni contraseñas.')}</p>
          <button className="btn-secondary" disabled={busy} onClick={chooseRoot}><FolderOpen className="h-4 w-4" />{t('Elegir raíz de OneDrive corporativo')}</button>
          {root && <p className="break-all rounded-xl bg-cream p-3 text-xs">{root}{bridgeFolder}</p>}
          <label className="flex gap-2 text-sm"><input type="checkbox" checked={syncAccount} onChange={e => setSyncAccount(e.target.checked)} />{t('Esta carpeta pertenece a mi OneDrive corporativo, está sincronizada y usaré esa misma cuenta en Microsoft.')}</label>
          <button className="btn-primary" disabled={busy || !root || !syncAccount || !snapshot || !ownerValid} onClick={() => void act({ kind: 'prepare', one_drive_root: root, calendar_name: '', owner_name: owner.trim() })}>{t('Preparar carpeta y solución')}</button>
        </div>}
        {phase === 'waiting_sign_in' && <div className="mt-5 space-y-4"><p className="text-sm">{t('Abre Microsoft e inicia sesión con tu cuenta corporativa. La sesión queda bajo el control del navegador; Atlas no recibe una señal de inicio de sesión.')}</p><button className="btn-primary" disabled={busy} onClick={portal}><ExternalLink className="h-4 w-4" />{t('Abrir inicio de sesión oficial')}</button><button className="btn-secondary ml-3" disabled={busy} onClick={() => void act({ kind: 'confirm_sign_in' })}>{t('Ya inicié sesión en Microsoft')}</button></div>}
        {phase === 'finding_environment' && <div className="mt-5 space-y-4"><p className="text-sm">{t('Esta instalación comenzó con una versión anterior de Atlas. Ya no hace falta copiar el identificador del entorno.')}</p><button className="btn-primary" disabled={busy} onClick={() => void act({ kind: 'confirm_environment', environment_id: '' })}>{t('Continuar sin identificador')}</button></div>}
        {phase === 'finding_connections' && <div className="mt-5 space-y-4"><p className="text-sm">{t('En Microsoft, revisa tus conexiones de')} <b>{connectors}</b>. {t('Durante la importación selecciona las que pertenezcan a tu cuenta. Si Microsoft solicita crear o reparar una conexión, completa esa pantalla oficial y vuelve aquí.')}</p><label className="flex items-start gap-2 text-sm"><input type="checkbox" checked={ownConnections} onChange={e => setOwnConnections(e.target.checked)} />{t('Confirmo que estas conexiones son mías, están autorizadas y OneDrive coincide con la carpeta elegida.')}</label><button className="btn-primary" disabled={busy || !ownConnections} onClick={() => void act({ kind: 'confirm_connections' })}>{t('Continuar a importación')}</button></div>}
        {phase === 'importing_solution' && <div className="mt-5 space-y-4">
          <p className="text-sm">{t('En Soluciones → Importar solución, carga el ZIP preparado por Atlas, asocia las conexiones con tu cuenta Circana y pulsa Importar. La solución es solo tuya: si ya la importaste antes se actualiza, y nunca modifica la de otros usuarios del entorno.')}</p>
          {session?.solutionDisplayName && <div className="flex items-center gap-3 rounded-xl border border-pine/20 bg-mint/40 p-4"><ClipboardCheck className="h-5 w-5 shrink-0 text-pine" /><div><p className="text-[11px] font-bold uppercase tracking-wider text-pine">{t('Nombre que verás en Power Automate')}</p><p className="mt-1 font-mono text-sm">{session.solutionDisplayName}</p></div></div>}
          <p className="break-all rounded-xl bg-cream p-3 text-xs">{snapshot?.packagePath}</p>
          <button className="btn-secondary" disabled={busy} onClick={() => void run(async () => { await api.installer.showPackage(role); })}><FolderOpen className="h-4 w-4" />{t('Mostrar ZIP')}</button><button className="btn-primary ml-3" disabled={busy} onClick={() => void act({ kind: 'confirm_import' })}>{t('Microsoft confirmó la importación')}</button>
        </div>}
        {phase === 'activating_flow' && <div className="mt-5 space-y-4"><p className="text-sm">{t('La configuración anterior esperaba una activación manual. El paquete nuevo ya se importa activo.')}</p><button className="btn-primary" disabled={busy} onClick={() => void act({ kind: 'confirm_active' })}>{t('Continuar a verificación')}</button></div>}
        {phase === 'verifying_file' && <div className="mt-5 space-y-4"><p className="break-all text-xs">{session?.folder}</p><p className="text-sm">{qa ? t('Atlas espera la respuesta de tu flujo QA a una solicitud de prueba. Puedes cerrar Atlas y reanudar después.') : t('Se acepta un paquete reciente del flujo personalizado para esta configuración. Puedes cerrar Atlas y reanudar después.')}</p>{session?.lastCheckedAt && <div className="rounded-xl bg-cream p-3 text-xs"><p><b>{t('Última comprobación:')}</b> {new Date(session.lastCheckedAt).toLocaleString()}</p><p className="mt-1 break-all"><b>{t('Archivo coincidente:')}</b> {session.lastCheckedFile ?? t('ninguno')}</p></div>}<div className="flex flex-wrap gap-3"><button className="btn-secondary" disabled={busy} onClick={() => void act({ kind: 'verify' })}><RotateCcw className="h-4 w-4" />{t('Comprobar ahora')}</button><button className="btn-secondary" disabled={busy} onClick={() => void run(async () => { await api.installer.openInbox(role); })}><FolderOpen className="h-4 w-4" />{t('Abrir carpeta')}</button></div></div>}
        {phase === 'completed' && <div className="mt-5 space-y-4"><p className="text-sm">{qa ? t('Tu conector QA está listo. Atlas empezará a importar el historial de QA de tu equipo en cuanto agregues a las personas que auditas.') : t('Usar esta carpeta cambia Atlas al modo Power Automate Inbox. Si tenías un destino SharePoint directo, tendrás que elegir su copia sincronizada local.')}</p><button className="btn-primary" disabled={busy} onClick={() => void run(async () => { if (session?.folder) await onUseFolder(session.folder); })}><Check className="h-4 w-4" />{t('Usar este conector en Atlas')}</button></div>}
        {phase === 'blocked_by_policy' && <div className="mt-5 space-y-4"><p className="text-sm">{t('No hay cambios locales que concedan este requisito corporativo. Conserva el modo local. Reanuda únicamente si el portal ya permite continuar.')}</p><button className="btn-secondary" disabled={busy} onClick={() => void act({ kind: 'retry' })}>{t('Volver al paso pendiente')}</button></div>}
        {!['completed', 'blocked_by_policy'].includes(phase) && <details className="mt-7 border-t border-ink/10 pt-4"><summary className="cursor-pointer text-sm font-bold">{t('Microsoft muestra un error o falta un requisito')}</summary><p className="mt-3 text-xs text-ink/55">{t('Introduce solo el código o una frase sin datos personales. Se analiza en memoria y se descarta; no se guarda ni se registra el texto.')}</p><textarea className="field mt-3" maxLength={4096} value={report} onChange={e => setReport(e.target.value)} placeholder={t('Ejemplo: prvImportCustomizations, AADSTS53003, DLP')} /><div className="mt-3 flex flex-wrap gap-2">{([['environment maker', 'Falta Environment Maker'], ['dataverse_required', 'No hay Dataverse'], ['outlook_access', 'Outlook bloqueado'], ...(qa ? [] : [['teams_access', 'Teams bloqueado']]), ['onedrive_access', 'OneDrive bloqueado'], ['license_required', 'Microsoft pide licencia']] as [string, string][]).map(([code, label]) => <button key={code} disabled={busy} className="btn-secondary text-xs" onClick={() => void act({ kind: 'report', message: code })}>{t(label)}</button>)}</div><button className="btn-secondary mt-3" disabled={busy || !report.trim()} onClick={() => { const message = report; setReport(''); void act({ kind: 'report', message }); }}>{t('Interpretar diagnóstico')}</button></details>}
        {error && <p role="alert" className="mt-5 rounded-xl bg-red-50 p-4 text-sm text-red-800">{error}</p>}
        {phase !== 'checking_requirements' && <div className="mt-7 flex flex-wrap items-center gap-3 border-t border-ink/10 pt-4"><button className="btn-secondary" disabled={busy} onClick={portal}><ExternalLink className="h-4 w-4" />{t('Volver a Microsoft')}</button><button className="text-xs font-bold text-pine underline" disabled={busy} onClick={() => setResetConfirm(!resetConfirm)}>{t('Preparar otra configuración')}</button>{resetConfirm && <div className="w-full rounded-xl bg-cream p-4 text-sm">{t('Esto reinicia solo el asistente y genera un nuevo identificador de instalación. El flujo existente seguirá en Microsoft y el nuevo ZIP creará una solución independiente; elimina la anterior en el portal si ya no la necesitas para evitar duplicados.')}<button className="btn-secondary mt-3 block" disabled={busy} onClick={() => { setResetConfirm(false); setOwnConnections(false); void act({ kind: 'reset' }); }}>{t('Reiniciar asistente')}</button></div>}</div>}
      </section>
    </div>
    <AtlasLoader show={(!snapshot && !error) || busy} message={t(!snapshot ? 'Cargando el asistente de Microsoft 365' : busyLabel)} detail={t(!snapshot ? 'Recuperando el progreso guardado y comprobando los recursos locales.' : 'Atlas conservará tu progreso si Microsoft necesita más tiempo.')} mode={!snapshot ? 'screen' : 'overlay'} delay={!snapshot ? 0 : 180} />
  </main>;
}

function localError(failure: unknown): string {
  const code = String(failure).replace('Atlas connector: ', '');
  const messages: Record<string, string> = {
    choose_onedrive_root: 'Selecciona una carpeta existente: la raíz de tu OneDrive corporativo.',
    inbox_not_writable: 'No se puede crear la carpeta AtlasBridge en esa ubicación. Revisa el acceso de tu usuario.',
    protected_location: 'Selecciona tu carpeta de OneDrive; no se escribirá en una ubicación protegida del sistema.',
    unsafe_inbox_link: 'La carpeta AtlasBridge apunta fuera de la carpeta seleccionada. Elige una carpeta sincronizada sin ese enlace.',
    settings_not_writable: 'Atlas no puede guardar el progreso en AppData. Comprueba los permisos de tu usuario.',
    browser_unavailable: 'No se pudo abrir el navegador. Abre https://make.powerautomate.com en tu navegador corporativo.',
    invalid_transition: 'El paso ya cambió. Cierra y vuelve a abrir el asistente para recuperar el estado actual.',
    package_missing: 'Prepara de nuevo la configuración para recuperar el paquete personalizado.',
    invalid_owner_name: 'Escribe tu nombre con letras (máximo 80 caracteres).',
  };
  return messages[code] ?? 'No se pudo completar la operación local. Cierra y reabre el asistente; no repitas la importación sin comprobar primero su resultado en Microsoft.';
}
