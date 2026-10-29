import type { SessionMessage } from "@/types";

export function messageVersionGroups(messages: SessionMessage[]) {
  const groups = new Map<string, SessionMessage[]>();
  for (const message of messages) {
    if (message.versionGroup && message.versionIndex !== undefined) {
      const versions = groups.get(message.versionGroup) ?? [];
      versions.push(message);
      groups.set(message.versionGroup, versions);
    }
  }
  return groups;
}

export function selectMessageVersions(messages: SessionMessage[], choices: Record<string, number>) {
  const groups = messageVersionGroups(messages);
  if (!groups.size) return messages;
  const selected = new Map([...groups].map(([id, versions]) => [id,
    versions.find(v => v.versionIndex === choices[id])
    ?? versions.find(v => v.versionSelected)
    // Wait for the native selection when a transcript page ends inside a group.
  ]));
  return messages.filter(message => !message.versionGroup || selected.get(message.versionGroup) === message);
}
