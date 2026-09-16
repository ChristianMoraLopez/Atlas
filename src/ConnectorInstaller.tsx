import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { Check, ExternalLink, FolderOpen, RotateCcw, ShieldCheck, X } from 'lucide-react';
import AtlasLoader from './AtlasLoader';

type Phase = 'checking_requirements' | 'waiting_sign_in' | 'finding_environment' | 'finding_connections' | 'importing_solution' | 'activating_flow' | 'verifying_file' | 'completed' | 'blocked_by_policy';
interface Session { phase: Phase; installationId: string; folder?: string; calendarName: string; environmentId?: string; diagnostic: string; lastCheckedAt?: string; lastCheckedFile?: string }
interface Snapshot { session: Session; packagePath: string; oneDriveRoot?: string; pacDetected: boolean }
const steps: [Phase, string][] = [
  ['checking_requirements', 'Comprobando requisitos'], ['waiting_sign_in', 'Esperando inicio de sesión'],
  ['finding_environment', 'Actualizando instalación anterior'], ['finding_connections', 'Conectando Microsoft 365'],
  ['importing_solution', 'Importando solución'], ['activating_flow', 'Actualizando flujo anterior'],
  ['verifying_file', 'Verificando archivo'], ['completed', 'Instalación completada'],
  ['blocked_by_policy', 'Bloqueado por política corporativa'],
];
const diagnostics: Record<string, string> = {
  portal_required: 'El portal oficial realiza la instalación. Atlas no puede leer tu sesión, detectar permisos remotos ni confirmar conexiones automáticamente.',
  user_reported_not_remotely_verified: 'Paso anterior confirmado por ti; Atlas no ha consultado el tenant. Continúa en el mismo entorno y con tu cuenta corporativa.',
  file_verified: 'Llegó un archivo de esta instalación y cumple atlas-evidence.schema.json. Esto verifica la entrega del puente; no certifica la integridad de todos tus datos de Microsoft 365.',
  previous_installation_detected: 'Atlas encontró evidencia de una instalación anterior. Importa el ZIP que preparó este asistente como actualización de AtlasBridge; los archivos antiguos no se borrarán.',
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

export default function ConnectorInstaller({ onClose, onUseFolder }: { onClose?: () => void; onUseFolder: (folder: string) => Promise<void> }) {
  const [snapshot, setSnapshot] = useState<Snapshot>();
  const [root, setRoot] = useState('');
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
  useEffect(() => {
    mounted.current = true;
    invoke<Snapshot>('connector_installer_status').then(s => {
      if (!mounted.current) return;
      setSnapshot(s); setRoot(s.oneDriveRoot ?? '');
    }).catch(() => setError('No se pudo cargar el asistente. Cierra y vuelve a abrirlo.'));
    return () => { mounted.current = false; };
  }, []);
  const run = async (operation: () => Promise<void>, label = 'Procesando la configuración') => {
    if (busyRef.current) return;
    busyRef.current = true; setBusyLabel(label); setBusy(true); setError('');
    try { await operation(); }
    catch (failure) { if (mounted.current) setError(localError(failure)); }
    finally { busyRef.current = false; if (mounted.current) setBusy(false); }
  };
  const actionLabels: Record<string, string> = {
    prepare: 'Preparando la solución AtlasBridge',
    confirm_sign_in: 'Guardando la confirmación de Microsoft',
    confirm_environment: 'Actualizando la configuración anterior',
    confirm_connections: 'Preparando la importación del conector',
    confirm_import: 'Confirmando la solución importada',
    confirm_active: 'Iniciando la comprobación del flujo',
    verify: 'Comprobando calendario, correo y Teams',
    report: 'Interpretando el diagnóstico de Microsoft',
    retry: 'Reanudando la configuración',
    reset: 'Preparando una configuración nueva',
  };
  const act = (action: Record<string, unknown>) => run(async () => {
    const next = await invoke<Snapshot>('connector_installer_action', { action });
    if (mounted.current) setSnapshot(next);
    if (action.kind === 'prepare') await invoke('connector_installer_open_portal');
  }, actionLabels[String(action.kind)] ?? 'Procesando la configuración');
  useEffect(() => {
    if (phase !== 'verifying_file') return;
    const poll = () => { void act({ kind: 'verify' }); };
    poll(); const timer = window.setInterval(poll, 15_000);
    return () => window.clearInterval(timer);
    // Only start/stop on a phase change; the ref prevents overlapping reads.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phase]);
  const portal = () => run(async () => { await invoke('connector_installer_open_portal'); }, 'Abriendo el portal de Microsoft');
  const chooseRoot = () => run(async () => {
    const folder = await open({ directory: true, multiple: false, title: 'Selecciona la raíz de tu OneDrive corporativo (no inbox)' });
    if (typeof folder === 'string') { setRoot(folder); setSyncAccount(false); }
  }, 'Abriendo tu OneDrive corporativo');
  return <main className="mx-auto min-h-screen max-w-6xl px-10 py-8" aria-busy={busy || !snapshot}>
    <header className="flex items-center justify-between"><span className="chip bg-mint text-pine"><ShieldCheck className="h-4 w-4" />Sin instalaciones de sistema</span>{onClose && <button className="btn-secondary" onClick={onClose} disabled={busy}><X className="h-4 w-4" />Cerrar y conservar progreso</button>}</header>
    <h1 className="mt-7 font-display text-4xl">Instalar conector de Microsoft 365</h1>
    <p className="mt-3 max-w-4xl text-sm leading-6 text-ink/65">Inicia sesión con tu cuenta Circana en el portal oficial, vincula tus conexiones e importa el paquete una sola vez. El flujo se entrega activo y se ejecuta cada cinco minutos; no necesitas abrir el diseñador ni copiar identificadores.</p>
    <div className="mt-7 grid grid-cols-[280px_1fr] gap-6">
      <ol className="card space-y-3 p-5" aria-label="Etapas de instalación">{steps.map(([key, label], i) => <li key={key} aria-current={key === phase ? 'step' : undefined} className={`flex gap-3 text-sm ${key === phase ? 'font-bold text-pine' : 'text-ink/50'}`}><span>{i + 1}.</span>{label}{key === 'completed' && phase === key && <Check className="h-4 w-4" />}</li>)}</ol>
      <section className="card min-w-0 p-7">
        <div role="status" aria-live="polite"><p className="text-xs uppercase tracking-wider text-pine">{index >= 1 && index <= 5 ? 'Esperando confirmación en el portal oficial' : 'Comprobación local de Atlas'}</p><h2 className="mt-2 font-display text-2xl">{steps[index][1]}</h2><p className="mt-3 text-sm leading-6 text-ink/65">{diagnostics[session?.diagnostic ?? 'portal_required'] ?? 'Revisa el diagnóstico del portal.'}</p></div>
        {phase === 'checking_requirements' && <div className="mt-5 space-y-4">
          <p className="text-sm">Necesitas un entorno con Dataverse, Environment Maker o permisos equivalentes para crear el flujo, privilegios de importación y conexiones propias de Outlook, Teams y OneDrive. Estos requisitos se comprueban en Microsoft; Atlas no los da por aprobados.</p>
          <p className="text-xs text-ink/55">PAC {snapshot?.pacDetected ? 'detectado, pero no se ejecutará' : 'no detectado; no hace falta'}. El asistente usa el navegador y el paquete incorporado. No guarda tokens ni contraseñas.</p>
          <button className="btn-secondary" disabled={busy} onClick={chooseRoot}><FolderOpen className="h-4 w-4" />Elegir raíz de OneDrive corporativo</button>
          {root && <p className="break-all rounded-xl bg-cream p-3 text-xs">{root}\AtlasBridge\inbox</p>}
          <label className="flex gap-2 text-sm"><input type="checkbox" checked={syncAccount} onChange={e => setSyncAccount(e.target.checked)} />Esta carpeta pertenece a mi OneDrive corporativo, está sincronizada y usaré esa misma cuenta en Microsoft.</label>
          <button className="btn-primary" disabled={busy || !root || !syncAccount || !snapshot} onClick={() => void act({ kind: 'prepare', one_drive_root: root, calendar_name: '' })}>Preparar carpeta y solución</button>
        </div>}
        {phase === 'waiting_sign_in' && <div className="mt-5 space-y-4"><p className="text-sm">Abre Microsoft e inicia sesión con tu cuenta corporativa. La sesión queda bajo el control del navegador; Atlas no recibe una señal de inicio de sesión.</p><button className="btn-primary" disabled={busy} onClick={portal}><ExternalLink className="h-4 w-4" />Abrir inicio de sesión oficial</button><button className="btn-secondary ml-3" disabled={busy} onClick={() => void act({ kind: 'confirm_sign_in' })}>Ya inicié sesión en Microsoft</button></div>}
        {phase === 'finding_environment' && <div className="mt-5 space-y-4"><p className="text-sm">Esta instalación comenzó con una versión anterior de Atlas. Ya no hace falta copiar el identificador del entorno.</p><button className="btn-primary" disabled={busy} onClick={() => void act({ kind: 'confirm_environment', environment_id: '' })}>Continuar sin identificador</button></div>}
        {phase === 'finding_connections' && <div className="mt-5 space-y-4"><p className="text-sm">En Microsoft, revisa tus conexiones de <b>Office 365 Outlook, Microsoft Teams y OneDrive for Business</b>. Durante la importación selecciona las que pertenezcan a tu cuenta. Si Microsoft solicita crear o reparar una conexión, completa esa pantalla oficial y vuelve aquí.</p><label className="flex items-start gap-2 text-sm"><input type="checkbox" checked={ownConnections} onChange={e => setOwnConnections(e.target.checked)} />Confirmo que las tres conexiones son mías, están autorizadas y OneDrive coincide con la carpeta elegida.</label><button className="btn-primary" disabled={busy || !ownConnections} onClick={() => void act({ kind: 'confirm_connections' })}>Continuar a importación</button></div>}
        {phase === 'importing_solution' && <div className="mt-5 space-y-4"><p className="text-sm">En <b>Soluciones → Importar solución</b>, carga el ZIP preparado por Atlas, asocia Outlook, Teams y OneDrive con tu cuenta Circana y pulsa <b>Importar</b>. El paquete actualiza <b>AtlasBridge</b> si ya existe y entrega el flujo activo.</p><p className="break-all rounded-xl bg-cream p-3 text-xs">{snapshot?.packagePath}</p><button className="btn-secondary" disabled={busy} onClick={() => void run(async () => { await invoke('connector_installer_show_package'); })}><FolderOpen className="h-4 w-4" />Mostrar ZIP</button><button className="btn-primary ml-3" disabled={busy} onClick={() => void act({ kind: 'confirm_import' })}>Microsoft confirmó la importación</button></div>}
        {phase === 'activating_flow' && <div className="mt-5 space-y-4"><p className="text-sm">La configuración anterior esperaba una activación manual. El paquete nuevo ya se importa activo.</p><button className="btn-primary" disabled={busy} onClick={() => void act({ kind: 'confirm_active' })}>Continuar a verificación</button></div>}
        {phase === 'verifying_file' && <div className="mt-5 space-y-4"><p className="break-all text-xs">{session?.folder}</p><p className="text-sm">Se acepta un paquete reciente del flujo personalizado para esta configuración. Puedes cerrar Atlas y reanudar después.</p>{session?.lastCheckedAt && <div className="rounded-xl bg-cream p-3 text-xs"><p><b>Última comprobación:</b> {new Date(session.lastCheckedAt).toLocaleString()}</p><p className="mt-1 break-all"><b>Archivo coincidente:</b> {session.lastCheckedFile ?? 'ninguno'}</p></div>}<div className="flex flex-wrap gap-3"><button className="btn-secondary" disabled={busy} onClick={() => void act({ kind: 'verify' })}><RotateCcw className="h-4 w-4" />Comprobar ahora</button><button className="btn-secondary" disabled={busy} onClick={() => void run(async () => { await invoke('connector_installer_open_inbox'); })}><FolderOpen className="h-4 w-4" />Abrir inbox</button></div></div>}
        {phase === 'completed' && <div className="mt-5 space-y-4"><p className="text-sm">Usar esta carpeta cambia Atlas al modo Power Automate Inbox. Si tenías un destino SharePoint directo, tendrás que elegir su copia sincronizada local.</p><button className="btn-primary" disabled={busy} onClick={() => void run(async () => { if (session?.folder) await onUseFolder(session.folder); })}><Check className="h-4 w-4" />Usar este conector en Atlas</button></div>}
        {phase === 'blocked_by_policy' && <div className="mt-5 space-y-4"><p className="text-sm">No hay cambios locales que concedan este requisito corporativo. Conserva el modo local. Reanuda únicamente si el portal ya permite continuar.</p><button className="btn-secondary" disabled={busy} onClick={() => void act({ kind: 'retry' })}>Volver al paso pendiente</button></div>}
        {!['completed', 'blocked_by_policy'].includes(phase) && <details className="mt-7 border-t border-ink/10 pt-4"><summary className="cursor-pointer text-sm font-bold">Microsoft muestra un error o falta un requisito</summary><p className="mt-3 text-xs text-ink/55">Introduce solo el código o una frase sin datos personales. Se analiza en memoria y se descarta; no se guarda ni se registra el texto.</p><textarea className="field mt-3" maxLength={4096} value={report} onChange={e => setReport(e.target.value)} placeholder="Ejemplo: prvImportCustomizations, AADSTS53003, DLP" /><div className="mt-3 flex flex-wrap gap-2">{[['environment maker', 'Falta Environment Maker'], ['dataverse_required', 'No hay Dataverse'], ['outlook_access', 'Outlook bloqueado'], ['teams_access', 'Teams bloqueado'], ['onedrive_access', 'OneDrive bloqueado'], ['license_required', 'Microsoft pide licencia']].map(([code, label]) => <button key={code} disabled={busy} className="btn-secondary text-xs" onClick={() => void act({ kind: 'report', message: code })}>{label}</button>)}</div><button className="btn-secondary mt-3" disabled={busy || !report.trim()} onClick={() => { const message = report; setReport(''); void act({ kind: 'report', message }); }}>Interpretar diagnóstico</button></details>}
        {error && <p role="alert" className="mt-5 rounded-xl bg-red-50 p-4 text-sm text-red-800">{error}</p>}
        {phase !== 'checking_requirements' && <div className="mt-7 flex flex-wrap items-center gap-3 border-t border-ink/10 pt-4"><button className="btn-secondary" disabled={busy} onClick={portal}><ExternalLink className="h-4 w-4" />Volver a Microsoft</button><button className="text-xs font-bold text-pine underline" disabled={busy} onClick={() => setResetConfirm(!resetConfirm)}>Preparar otra configuración</button>{resetConfirm && <div className="w-full rounded-xl bg-cream p-4 text-sm">Esto reinicia solo el asistente y su identificador de verificación. El flujo existente seguirá en Microsoft; actualiza AtlasBridge con el nuevo ZIP para evitar duplicados.<button className="btn-secondary mt-3 block" disabled={busy} onClick={() => { setResetConfirm(false); setOwnConnections(false); void act({ kind: 'reset' }); }}>Reiniciar asistente</button></div>}</div>}
      </section>
    </div>
    <AtlasLoader show={(!snapshot && !error) || busy} message={!snapshot ? 'Cargando el asistente de Microsoft 365' : busyLabel} detail={!snapshot ? 'Recuperando el progreso guardado y comprobando los recursos locales.' : 'Atlas conservará tu progreso si Microsoft necesita más tiempo.'} mode={!snapshot ? 'screen' : 'overlay'} delay={!snapshot ? 0 : 180} />
  </main>;
}

function localError(failure: unknown): string {
  const code = String(failure).replace('Atlas connector: ', '');
  const messages: Record<string, string> = {
    choose_onedrive_root: 'Selecciona una carpeta existente: la raíz de tu OneDrive corporativo.',
    inbox_not_writable: 'No se puede crear AtlasBridge/inbox en esa carpeta. Revisa el acceso de tu usuario.',
    protected_location: 'Selecciona tu carpeta de OneDrive; no se escribirá en una ubicación protegida del sistema.',
    unsafe_inbox_link: 'AtlasBridge/inbox apunta fuera de la carpeta seleccionada. Elige una carpeta sincronizada sin ese enlace.',
    settings_not_writable: 'Atlas no puede guardar el progreso en AppData. Comprueba los permisos de tu usuario.',
    browser_unavailable: 'No se pudo abrir el navegador. Abre https://make.powerautomate.com en tu navegador corporativo.',
    invalid_transition: 'El paso ya cambió. Cierra y vuelve a abrir el asistente para recuperar el estado actual.',
    package_missing: 'Prepara de nuevo la configuración para recuperar el paquete personalizado.',
  };
  return messages[code] ?? 'No se pudo completar la operación local. Cierra y reabre el asistente; no repitas la importación sin comprobar primero su resultado en Microsoft.';
}
