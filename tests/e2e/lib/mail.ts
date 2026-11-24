/**
 * Reads the emails the app sent, to follow their links like a person would.
 *  - Local runs: Mailpit (server/local catches every email there).
 *  - A server whose email still goes to its log (MAIL=log): TP_MAIL_LOG_CMD prints the
 *    recent web app logs, e.g. an ssh command running `docker compose logs`.
 */
import { execSync } from 'node:child_process';

export interface Mailbox {
    /** The first link matching `pattern` in an email to `to` sent after `since` (ms). */
    link(to: string, pattern: RegExp, since: number): Promise<string>;
    /** The subject of the newest email to `to`. */
    subject(to: string): Promise<string>;
}

interface MailpitSummary {
    ID: string;
    Created: string;
    Subject: string;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export function mailpit(base = process.env.TP_MAILPIT_URL ?? 'http://localhost:8025'): Mailbox {
    const search = async (to: string) => {
        const res = await fetch(`${base}/api/v1/search?query=${encodeURIComponent(`to:"${to}"`)}`);
        return ((await res.json()) as { messages: MailpitSummary[] }).messages;
    };
    return {
        async link(to, pattern, since) {
            for (let attempt = 0; attempt < 40; attempt++) {
                for (const m of (await search(to)).filter((x) => Date.parse(x.Created) >= since - 5000)) {
                    const full = (await (await fetch(`${base}/api/v1/message/${m.ID}`)).json()) as { HTML: string };
                    const hrefs = [...full.HTML.matchAll(/href="([^"]+)"/g)].map((x) => x[1].replace(/&amp;/g, '&'));
                    const hit = hrefs.find((h) => pattern.test(h));
                    if (hit) return hit;
                }
                await sleep(1000);
            }
            throw new Error(`No email to ${to} with a link matching ${pattern}`);
        },
        async subject(to) {
            return (await search(to))[0]?.Subject ?? '';
        },
    };
}

/** Emails written to the log by Laravel's log mailer (raw MIME, quoted-printable). */
export function logMailbox(command: string): Mailbox {
    const messages = (to: string) => {
        const raw = execSync(command, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
        const text = raw.replace(/=\r?\n/g, '').replace(/=3D/g, '=').replace(/&amp;/g, '&');
        // One block per email, starting at its From header.
        return text
            .split(/\n(?=From: )/)
            .filter((block) => block.toLowerCase().includes(`to: ${to.toLowerCase()}`) || block.toLowerCase().includes(`<${to.toLowerCase()}>`));
    };
    return {
        async link(to, pattern) {
            for (let attempt = 0; attempt < 30; attempt++) {
                for (const block of messages(to).reverse()) {
                    const links = block.match(/https?:\/\/[^\s"'<>)\]]+/g) ?? [];
                    const hit = links.find((l) => pattern.test(l));
                    if (hit) return hit;
                }
                await sleep(2000);
            }
            throw new Error(`No logged email to ${to} with a link matching ${pattern}`);
        },
        async subject(to) {
            const last = messages(to).at(-1) ?? '';
            return last.match(/^Subject: (.*)$/m)?.[1]?.trim() ?? '';
        },
    };
}

export const mailbox = (): Mailbox =>
    process.env.TP_MAIL_LOG_CMD ? logMailbox(process.env.TP_MAIL_LOG_CMD) : mailpit();
