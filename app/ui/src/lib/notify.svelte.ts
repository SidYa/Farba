// Transient messages shown in the status bar (errors stay until replaced).

export const message = $state<{ text: string; kind: "info" | "error" }>({ text: "", kind: "info" });

let timer: ReturnType<typeof setTimeout> | undefined;

export function notify(text: string, kind: "info" | "error" = "info") {
  message.text = text;
  message.kind = kind;
  clearTimeout(timer);
  if (kind === "info") timer = setTimeout(() => (message.text = ""), 5000);
}
