# Asistente «Instalar conector de Microsoft 365»

Atlas instala el puente mediante la experiencia oficial de importación de soluciones de Power Automate. El asistente prepara los archivos locales y guía las confirmaciones que Microsoft exige; no controla el navegador ni obtiene acceso a la sesión del usuario.

## Primera ejecución en el PC corporativo

1. Extrae el ZIP portátil completo y ejecuta `Atlas.exe`. No se necesita instalación ni elevación UAC.
2. Pulsa **Instalar conector de Microsoft 365**.
3. Selecciona la raíz del OneDrive corporativo que ya está sincronizado. Atlas crea `AtlasBridge\inbox\scheduled`, `inbox\teams`, `inbox\requested` y `AtlasBridge\requests`.
4. Atlas abre `https://make.powerautomate.com/` en el navegador predeterminado. Completa el inicio de sesión o MFA con la cuenta Circana si Microsoft lo solicita.
5. Confirma que tienes conexiones propias de **Office 365 Outlook**, **Microsoft Teams** y **OneDrive for Business**. La cuenta de OneDrive debe ser la que sincroniza la carpeta elegida. La solución instala juntos el flujo programado y la captura de Teams por mensaje.
6. En **Soluciones → Importar solución**, selecciona el archivo `AtlasBridge_1_0_0_0.zip` preparado por Atlas. Asocia cada referencia con tu conexión y confirma la importación. No copies identificadores de tenant, entorno o aplicación.
7. La solución 1.11 se entrega activa y se ejecuta cada cinco minutos. Si ya existe `AtlasBridge`, importa como actualización de esa misma solución. Esta versión continúa con el día actual cuando `AtlasBridge/requests/selected-date.txt` todavía no existe o está vacío.
8. Espera la ejecución programada y la sincronización de OneDrive. Marca `AtlasBridge` como **Siempre mantener en este dispositivo**. Atlas comprueba cada 15 segundos si aparece un JSON nuevo compatible con `atlas-evidence.schema.json`.
9. Cuando el asistente muestre **Instalación completada**, pulsa **Usar este conector en Atlas**. Si no existe, Atlas crea `Tracker_Circana.xlsx` en la raíz de OneDrive elegida.

La recurrencia es de cinco minutos, por lo que la primera comprobación puede tardar ese tiempo más la sincronización de OneDrive. La comprobación acepta únicamente un paquete del recolector programado cuyo nombre incluya el identificador aleatorio de esa instalación y cuyo estado confirme calendario, correo y Teams. Un JSON de ejemplo, un paquete de la captura inmediata de Teams o un archivo de una instalación anterior no puede completar el asistente.

## Interacciones inevitables

Microsoft puede exigir inicio de sesión, MFA, selección o creación de conexiones y confirmación de la importación. Atlas no afirma que esos pasos sean silenciosos. El navegador puede reutilizar el SSO corporativo por decisión de Microsoft, pero Atlas no lee cookies, perfiles ni cachés de tokens.

Atlas no puede comprobar directamente los permisos remotos porque hacerlo requeriría autenticar otra aplicación o usar interfaces privadas. Las etapas del portal aparecen como confirmadas por el usuario. La única comprobación automática de extremo a extremo es la llegada local de un archivo válido producido por el flujo personalizado.

## Requisitos y bloqueos que el asistente identifica

- Un entorno con una base de datos Dataverse utilizable. Los clientes Microsoft 365 pueden crear flujos dentro de soluciones cuando el entorno ya tiene Dataverse; adjuntar Dataverse a otro entorno es una operación administrativa.
- Environment Maker o permisos equivalentes para crear el flujo y sus referencias, además de privilegios Dataverse para leer/crear soluciones e importar personalizaciones. El asistente reconoce nombres como `prvImportCustomizations`, `prvCreateWorkflow`, `prvCreateConnectionReference`, `prvReadSolution` y `prvCreateSolution`.
- Una licencia de Microsoft 365 o Power Automate que permita flujos con conectores estándar, sujeta a las asignaciones y políticas del tenant.
- Acceso legítimo del usuario a las conexiones de Outlook, Teams y OneDrive. No se aceptan identificadores de conexión copiados de otra cuenta.
- Ausencia de una política DLP, acceso condicional o bloqueo de soluciones no administradas que impida este escenario.

Si aparece uno de esos bloqueos, Atlas conserva el modo local y muestra el requisito concreto que el portal comunicó. Un error `403` aislado se deja sin clasificar porque no demuestra por sí solo qué rol falta.

## Seguridad y almacenamiento

- El instalador guarda en `%APPDATA%\com.capgemini.atlas-tracker\connector-installer` únicamente etapa, identificador aleatorio y rutas locales. Los campos antiguos de calendario o entorno se ignoran al reanudar una instalación previa.
- El texto que el usuario pega para clasificar un error se procesa en memoria, se reduce a un código conocido y se descarta. No se guarda ni se escribe en logs.
- El ZIP contiene referencias lógicas a tres conectores estándar y no incluye IDs de conexiones, creador, tenant, secretos ni tokens.
- Atlas solo abre la página principal HTTPS documentada de Power Automate. No construye llamadas a endpoints internos ni automatiza clics, contraseñas, consentimiento o MFA.
- Reintentar una comprobación no vuelve a importar. Antes de importar otra vez se debe revisar si `AtlasBridge` ya existe y actualizar esa misma solución para evitar duplicados.

## Desarrollo y validación del artefacto

`convert-solution.mjs` convierte la definición heredada a fuentes de solución, elimina metadatos de usuario, añade tres referencias lógicas, marca el flujo como activo y registra el estado de cada origen. El archivo se crea incluso si un conector falla, para que Atlas pueda mostrar el dato faltante y permitir una entrada manual real. `build-solution.ps1` usa PAC únicamente para empaquetar; no autentica ni importa.

```powershell
.\power-automate\build-solution.ps1 -PacPath C:\ruta-del-desarrollador\pac.exe
.\power-automate\validate-solution.ps1
```

PAC no se distribuye con Atlas. Sus términos conceden derechos generales de instalación y uso y derechos de distribución limitados a los elementos que Microsoft identifica como redistribuibles; no conceden permiso general para redistribuir `pac.exe`. El PC final no necesita PAC, .NET, Node.js, Rust, Visual Studio ni Power Platform CLI.

El ZIP portátil tampoco incluye `Atlas-Export-Evidence.zip`; ese paquete heredado se conserva en el repositorio únicamente como compatibilidad y referencia de desarrollo. La entrega al usuario contiene solo `AtlasBridge_1_0_0_0.zip`, que no lleva los metadatos de creador o tenant presentes en la exportación heredada.

La validación local comprueba la identidad y versión de la solución, el flujo, la correspondencia de sus tres referencias, los tipos de acciones permitidos, la ruta de OneDrive, la correlación de instalación, la ausencia de IDs/secretos y la igualdad entre el JSON empaquetado y las fuentes revisadas. Una importación real solo se valida en un entorno autorizado y con interacción del usuario.

## Referencias oficiales verificadas

- [Importar una solución en Power Automate](https://learn.microsoft.com/power-automate/import-flow-solution)
- [Referencias de conexión en soluciones](https://learn.microsoft.com/power-apps/maker/data-platform/create-connection-reference)
- [Importar soluciones y seleccionar conexiones](https://learn.microsoft.com/power-apps/maker/data-platform/import-update-export-solutions)
- [Power Platform CLI: instalación y perfiles de autenticación](https://learn.microsoft.com/power-platform/developer/cli/introduction)
- [Comandos oficiales de solución de PAC](https://learn.microsoft.com/power-platform/developer/cli/reference/solution)
- [Preguntas de licenciamiento de Power Automate](https://learn.microsoft.com/power-platform/admin/power-automate-licensing/faqs)
- [Términos de licencia de Microsoft Power Apps CLI](https://www.microsoft.com/business-applications/legal/slt-powerapps-cli/)
