/**
 * Reads the emails the app sent, to follow their links like a person would.
 * Local runs use Mailpit (server/local catches every email there).
 */
export interface Mailbox {
    /** The first link matching `pattern` in an email to `to` sent after `since` (ms). */
    link(to: string, pattern: RegExp, since: number): Promise<string>;
}

interface MailpitSummary {
    ID: string;
    Created: string;
    Subject: string;
}

export function mailpit(base = process.env.TP_MAILPIT_URL ?? 'http://localhost:8025'): Mailbox {
    return {
        async link(to, pattern, since) {
            for (let attempt = 0; attempt < 40; attempt++) {
                const query = encodeURIComponent(`to:"${to}"`);
                const res = await fetch(`${base}/api/v1/search?query=${query}`);
                const { messages } = (await res.json()) as { messages: MailpitSummary[] };
                for (const m of messages.filter((x) => Date.parse(x.Created) >= since - 5000)) {
                    const full = (await (await fetch(`${base}/api/v1/message/${m.ID}`)).json()) as {
                        HTML: string;
                    };
                    const hrefs = [...full.HTML.matchAll(/href="([^"]+)"/g)].map((x) =>
                        x[1].replace(/&amp;/g, '&')
                    );
                    const hit = hrefs.find((h) => pattern.test(h));
                    if (hit) return hit;
                }
                await new Promise((r) => setTimeout(r, 1000));
            }
            throw new Error(`No email to ${to} with a link matching ${pattern}`);
        },
    };
}

/** The subject of the newest email to `to` (to check what people see in their inbox). */
export async function latestSubject(to: string, base = process.env.TP_MAILPIT_URL ?? 'http://localhost:8025') {
    const res = await fetch(`${base}/api/v1/search?query=${encodeURIComponent(`to:"${to}"`)}`);
    const { messages } = (await res.json()) as { messages: MailpitSummary[] };
    return messages[0]?.Subject ?? '';
}
