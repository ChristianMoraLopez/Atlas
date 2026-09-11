# Puente Power Automate → Atlas → Excel

Este modo evita por completo el registro de una aplicación en Microsoft Entra. Power Automate usa las conexiones corporativas que ya tienes, crea un paquete JSON en OneDrive y el cliente de OneDrive lo sincroniza al PC. Atlas lee ese paquete, actualiza el tracker local y OneDrive vuelve a publicar el Excel.

No hay un segundo flujo de subida: escribir directamente en la copia sincronizada del tracker es más simple y evita carreras en las que un flujo podría sobrescribir una versión más nueva.

## Qué incluye esta carpeta

- `AtlasBridge_1_0_0_0.zip`: solución no administrada AtlasBridge 1.3 para el asistente actual, con referencias de conexión de Outlook, Teams y OneDrive. El nombre estable permite que el cliente la encuentre; la versión interna controla la actualización.
- `solution-source/`: fuente revisable de la solución actual.
- `INSTALLER.md`: primera ejecución, límites y diagnóstico del asistente.
- `Atlas-Export-Evidence.zip`: paquete heredado conservado solo como referencia de desarrollo; no produce el contrato v2 actual.
- `package-source/`: fuente legible del paquete anterior.
- `atlas-evidence.schema.json`: contrato exacto que valida Atlas.
- `atlas-evidence.example.json`: ejemplo válido para probar la aplicación sin Microsoft 365.

## Requisitos reales

- Para el asistente actual, un entorno con Dataverse y permisos para crear el flujo, crear referencias e importar soluciones. Environment Maker puede ser necesario, pero no garantiza por sí solo todos los privilegios Dataverse.
- Conexiones estándar de **Office 365 Outlook**, **Microsoft Teams** y **OneDrive for Business**. Las acciones del flujo no usan HTTP premium, conectores personalizados, el conector Dataverse, una aplicación Entra ni secretos; Dataverse sí es el contenedor requerido para importar la solución.
- OneDrive corporativo iniciado en Windows.

Las credenciales no vienen dentro del ZIP. Al importarlo, Power Automate obliga a asociar cada referencia con tus propias conexiones; eso es normal y evita distribuir tokens.

## Instalación recomendada

Usa **Instalar conector de Microsoft 365** dentro de Atlas y sigue `INSTALLER.md`. El asistente genera un ZIP personalizado para correlacionar la primera evidencia, abre el portal oficial y verifica el resultado local. No solicita IDs de aplicación, tenant, entorno o calendario. El usuario todavía debe completar el inicio de sesión, la asociación de conexiones y la confirmación de importación que muestra Microsoft.

El flujo se importa activo, usa el primer calendario devuelto por Outlook y se ejecuta cada hora en la zona `SA Pacific Standard Time` (Bogotá). Calendario, correo y Teams se ejecutan de forma independiente. Las consultas de correo y Teams están limitadas al día para evitar el timeout HTTP de Logic Apps. Aunque falle una consulta, el flujo escribe el paquete con una marca de estado para que Atlas muestre la alerta correspondiente y acepte los datos parciales.

## Configuración de Atlas

1. Espera a que el icono de OneDrive indique que terminó de sincronizar.
2. Abre Atlas y, en la pantalla inicial, pulsa **Choose Power Automate inbox**.
3. Selecciona la carpeta local sincronizada, por ejemplo:

   `C:\Users\TU_USUARIO\OneDrive - Circana\AtlasBridge\inbox`

4. Completa el perfil.
5. Como destino, elige el `.xlsx` o `.xlsm` dentro de una carpeta sincronizada de OneDrive o de una biblioteca de SharePoint sincronizada con OneDrive.
6. Deja **Automatic inbox import** activo. Atlas importará el paquete más reciente del día al abrirse y luego cada hora.

Para probar sin esperar el flujo, copia `atlas-evidence.example.json` dentro de `inbox`, cambia `targetDate` y las fechas de ejemplo al día elegido y pulsa **Import**.

## Qué recoge y qué no

- Calendario: asunto, horas, organizador e identificador. Las reuniones válidas quedan seleccionadas para Excel.
- Correo: asunto, remitente, hora e identificador; no se guardan cuerpos ni adjuntos. Cada correo queda sin seleccionar hasta que lo confirmes en Atlas.
- Teams: texto, autor, hora, chat e identificador. Atlas interpreta esa evidencia con la IA local incluida y mantiene cada sugerencia sin seleccionar hasta que el usuario la revise. Si la IA no está disponible, conserva una agrupación determinista de los mensajes para que la evidencia no se pierda.
- Atlas no modifica, elimina ni mueve elementos en Outlook o Teams.

El flujo consulta hasta 500 eventos, los 1000 correos más recientes y hasta 50 mensajes recientes por chat. El conector **List chats** solo enumera chats recientes; esto replica el alcance práctico del extractor actual y no es un archivo histórico completo de Teams.

## Si Circana oculta “Importar paquete”

La importación de paquetes puede estar deshabilitada aunque sí puedas crear flujos. En ese caso crea un flujo programado y replica `package-source/Microsoft.Flow/flows/8e5c1f84-dcbb-4a2c-9d2f-62e9c38105d2/definition.json` en el diseñador. Los nombres de acciones y expresiones están ahí completos. El resultado final debe respetar `atlas-evidence.schema.json` y escribirse en `/AtlasBridge/inbox`.

No hay forma legítima de empaquetar la autenticación corporativa dentro del flujo: siempre tendrás que escoger tus conexiones al importar. Eso no es permiso de una app nueva; son las mismas conexiones delegadas que Power Automate ya usa con tu correo.
