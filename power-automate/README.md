# Puente Power Automate → Atlas → Excel

Este modo evita por completo el registro de una aplicación en Microsoft Entra. Power Automate usa las conexiones corporativas que ya tienes, crea un paquete JSON en OneDrive y el cliente de OneDrive lo sincroniza al PC. Atlas lee ese paquete, actualiza el tracker local y OneDrive vuelve a publicar el Excel.

No hay un segundo flujo de subida: escribir directamente en la copia sincronizada del tracker es más simple y evita carreras en las que un flujo podría sobrescribir una versión más nueva.

## Qué incluye esta carpeta

- `AtlasBridge_1_0_0_0.zip`: solución no administrada AtlasBridge 1.8 para el asistente actual. Instala dos flujos activos con las mismas referencias de conexión de Outlook, Teams y OneDrive. El nombre estable permite que el cliente la encuentre; la versión interna controla la actualización.
- `solution-source/`: fuente revisable de la solución actual.
- `INSTALLER.md`: primera ejecución, límites y diagnóstico del asistente.
- `Atlas-Export-Evidence.zip`: paquete heredado conservado solo como referencia de desarrollo; no produce el contrato v3 actual.
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

La solución importa dos flujos activos con fechas calculadas en la zona `SA Pacific Standard Time` (Bogotá):

- **Atlas - Capture Teams messages** se dispara con cada mensaje nuevo de un chat del usuario, consulta directamente ese chat y escribe evidencia incremental en OneDrive. Así la captura diaria no depende de enumerar el historial completo de chats.
- **Atlas - Export evidence to OneDrive** se ejecuta cada 15 minutos para correo y calendario. También conserva un respaldo de Teams limitado a los 12 chats más recientes actualizados hoy, espera cinco segundos entre consultas (el mínimo aceptado por Power Automate), reintenta fallos transitorios y solicita hasta 20 mensajes del día por chat. La captura por evento sigue guardando mensajes de los demás chats sin recorrer una lista de 100 conversaciones.

Atlas combina los paquetes del mismo día y elimina mensajes de Teams repetidos. Si una fuente falla, el flujo programado escribe las demás con una marca de estado para que Atlas muestre la alerta correspondiente y acepte datos parciales.

## Configuración de Atlas

1. Espera a que el icono de OneDrive indique que terminó de sincronizar.
2. Abre Atlas y, en la pantalla inicial, pulsa **Choose Power Automate inbox**.
3. Selecciona la carpeta local sincronizada, por ejemplo:

   `C:\Users\TU_USUARIO\OneDrive - Circana\AtlasBridge\inbox`

4. Completa el perfil.
5. Como destino, elige el `.xlsx` o `.xlsm` dentro de una carpeta sincronizada de OneDrive o de una biblioteca de SharePoint sincronizada con OneDrive.
6. Deja activo **Daily automatic tracker**. Windows abrirá Atlas a las 17:30, o a la hora que elijas, y la aplicación escribirá todas las actividades reales encontradas.

Para probar sin esperar el flujo, copia `atlas-evidence.example.json` dentro de `inbox`, cambia `targetDate` y las fechas de ejemplo al día elegido y pulsa **Import**.

## Qué recoge y qué no

- Calendario: asunto, horas, organizador e identificador. Las reuniones válidas quedan seleccionadas para Excel.
- Correo: bandeja de entrada y enviados, asunto, vista previa del contenido, participantes, hora e identificador; nunca se descargan adjuntos. La IA local convierte únicamente trabajo realizado en tareas separadas.
- Teams: texto, autor, hora, chat e identificador. La IA local puede obtener varias tareas independientes de una conversación. Recibir o enviar un mensaje, por sí solo, no se registra como actividad.
- Atlas no modifica, elimina ni mueve elementos en Outlook o Teams.

El flujo programado consulta hasta 500 eventos, los 100 correos más recientes y hasta 20 mensajes del día por cada chat reciente que entregue el conector. **List chats** no expone paginación ni un filtro por fecha en su acción estándar, por lo que esa ruta sigue siendo solo un respaldo. La cobertura principal de Teams usa el disparador por mensaje y consulta el chat identificado por el evento, aunque el usuario participe en más de 100 conversaciones durante el día.

## Si Circana oculta “Importar paquete”

La importación de paquetes puede estar deshabilitada aunque sí puedas crear flujos. En ese caso crea un flujo programado y replica `package-source/Microsoft.Flow/flows/8e5c1f84-dcbb-4a2c-9d2f-62e9c38105d2/definition.json` en el diseñador. Los nombres de acciones y expresiones están ahí completos. El resultado final debe respetar `atlas-evidence.schema.json` y escribirse en `/AtlasBridge/inbox`.

No hay forma legítima de empaquetar la autenticación corporativa dentro del flujo: siempre tendrás que escoger tus conexiones al importar. Eso no es permiso de una app nueva; son las mismas conexiones delegadas que Power Automate ya usa con tu correo.
