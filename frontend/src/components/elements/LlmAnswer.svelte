<script lang="ts">
    import { Paper, Text, Loader, Group, Button } from "@svelteuidev/core"
    import { llmTurn, translations, translate, stopSpeaking } from "@/stores"

    // shown next to a finished answer, because that is exactly when the
    // assistant starts reading it out. It stays until the next question, which
    // means it can outlive the speech - pressing it then is harmless, and that
    // is the better failure than having no way to stop a two-minute answer.
    let silenced = false
    $: if ($llmTurn?.thinking) silenced = false

    function silence() {
        silenced = stopSpeaking() || silenced
    }

    $: t = (key: string) => translate($translations, key)

    // localized headline from the machine-readable code, English detail
    // underneath. LlmError::code() uses snake_case, the FTL keys use kebab.
    $: headline = $llmTurn?.errorCode
        ? t(`llm-error-${$llmTurn.errorCode.replace(/_/g, "-")}`)
        : t("llm-answer")
</script>

{#if $llmTurn}
    <div class="llm-panel">
        <Paper shadow="xs" radius="sm" padding="sm" withBorder>
            {#if $llmTurn.thinking}
                <Group spacing="xs">
                    <Loader size="sm" color="cyan" />
                    <Text size="sm" color="gray">{t('llm-thinking')}</Text>
                </Group>
                {#if $llmTurn.prompt}
                    <div class="llm-prompt">
                        <Text size="xs" color="gray">{$llmTurn.prompt}</Text>
                    </div>
                {/if}
            {:else}
                <Text size="xs" weight="bold" color={$llmTurn.errorCode ? "red" : "cyan"}>
                    {headline}
                    {#if !$llmTurn.errorCode && $llmTurn.elapsedMs}
                        <span class="llm-meta">
                            {($llmTurn.elapsedMs / 1000).toFixed(1)}s · {$llmTurn.model}
                        </span>
                    {/if}
                </Text>
                {#if !$llmTurn.errorCode && $llmTurn.answer && !silenced}
                    <div class="llm-silence">
                        <Button size="xs" variant="subtle" color="gray" on:click={silence}>
                            {t('llm-stop-speaking')}
                        </Button>
                    </div>
                {/if}
                <!-- the question, always. an answer arrives seconds after the
                     utterance and the panel outlives the recognized-text line,
                     so an answer with no question on it is unreadable as soon
                     as anything else has happened. -->
                {#if $llmTurn.prompt}
                    <div class="llm-prompt">
                        <Text size="xs" color="gray">{$llmTurn.prompt}</Text>
                    </div>
                {/if}
                <div class="llm-body">
                    <Text size="sm">{$llmTurn.answer ?? $llmTurn.error ?? ""}</Text>
                </div>
            {/if}
        </Paper>
    </div>
{/if}

<style>
    /* the window is 550x800 and not resizable (crates/jarvis-gui/tauri.conf.json),
       body has overflow:hidden (css/main.scss) and the reactor alone is 300px -
       an unbounded answer would push the Stats bar off screen with no way to
       scroll it back. this panel scrolls inside itself instead. */
    .llm-panel {
        width: 100%;
        max-height: 170px;
        overflow-y: auto;
        margin: 0 0 0.5rem;
    }

    .llm-prompt {
        margin-top: 0.2rem;
        opacity: 0.75;
        font-style: italic;
        word-break: break-word;
    }

    .llm-body {
        margin-top: 0.35rem;
        /* the model answers in prose with newlines; Text does not preserve them */
        white-space: pre-wrap;
        word-break: break-word;
    }

    .llm-silence {
        margin-top: 0.25rem;
    }

    .llm-meta {
        float: right;
        font-weight: normal;
        opacity: 0.6;
    }
</style>
