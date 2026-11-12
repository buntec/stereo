import { useEffect, useState } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { List, ListItem } from "@astryxdesign/core/List";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Spinner } from "@astryxdesign/core/Spinner";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Grid } from "@astryxdesign/core/Grid";
import type {
  INewReleasesCandidate,
  RequestReply,
  ServerMsg,
} from "./Types.tsx";

/// Names whose selection differs from the server's default, remembered per collection so a
/// deselected artist stays deselected on the next search. Names that drop out of the
/// candidate list keep their override in case they come back.
export type NewReleasesOverrides = {
  artists: Record<string, boolean>;
  labels: Record<string, boolean>;
};

type Candidates = {
  artists: INewReleasesCandidate[];
  labels: INewReleasesCandidate[];
};

type NewReleasesDialogProps = {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  days: number;
  requestReply: RequestReply;
  overrides?: NewReleasesOverrides;
  onSearch: (
    artists: string[],
    labels: string[],
    overrides: NewReleasesOverrides,
  ) => void;
};

function initialSelection(
  candidates: INewReleasesCandidate[],
  overrides: Record<string, boolean> = {},
): Set<string> {
  return new Set(
    candidates
      .filter((c) => overrides[c.name] ?? c.is_default)
      .map((c) => c.name),
  );
}

function nextOverrides(
  candidates: INewReleasesCandidate[],
  selected: Set<string>,
  prev: Record<string, boolean> = {},
): Record<string, boolean> {
  const next = { ...prev };
  for (const c of candidates) {
    const isSelected = selected.has(c.name);
    if (isSelected === c.is_default) {
      delete next[c.name];
    } else {
      next[c.name] = isSelected;
    }
  }
  return next;
}

function describeCounts(c: INewReleasesCandidate): string {
  return c.five_star > 0
    ? `${c.four_plus} rated 4★+, ${c.five_star} rated 5★`
    : `${c.four_plus} rated 4★+`;
}

function NameColumn({
  title,
  candidates,
  selected,
  setSelected,
  onSubmit,
}: {
  title: string;
  candidates: INewReleasesCandidate[];
  selected: Set<string>;
  setSelected: (selected: Set<string>) => void;
  onSubmit: () => void;
}) {
  const [filter, setFilter] = useState("");
  const needle = filter.trim().toLowerCase();
  const visible = needle
    ? candidates.filter((c) => c.name.toLowerCase().includes(needle))
    : candidates;

  const setVisible = (checked: boolean) => {
    const next = new Set(selected);
    for (const c of visible) {
      if (checked) {
        next.add(c.name);
      } else {
        next.delete(c.name);
      }
    }
    setSelected(next);
  };

  const toggle = (name: string, checked: boolean) => {
    const next = new Set(selected);
    if (checked) {
      next.add(name);
    } else {
      next.delete(name);
    }
    setSelected(next);
  };

  return (
    <VStack gap={2} width="100%">
      <HStack gap={2} vAlign="center" hAlign="between" wrap="wrap">
        <Text type="label" weight="bold">
          {title}
        </Text>
        <Text type="supporting" color="secondary">
          {selected.size} of {candidates.length} selected
        </Text>
      </HStack>
      <TextInput
        label={`Filter ${title.toLowerCase()}`}
        isLabelHidden
        placeholder={`Filter ${title.toLowerCase()}…`}
        hasClear
        value={filter}
        onChange={setFilter}
        onKeyDown={(ev) => {
          if (ev.key === "Enter") onSubmit();
        }}
      />
      <HStack gap={2}>
        <Button
          label="Select all"
          variant="ghost"
          size="sm"
          onClick={() => setVisible(true)}
        />
        <Button
          label="Select none"
          variant="ghost"
          size="sm"
          onClick={() => setVisible(false)}
        />
      </HStack>
      <VStack isScrollable style={{ maxHeight: "45vh" }}>
        {visible.length === 0 ? (
          <Text type="supporting" color="secondary">
            {candidates.length === 0
              ? `No rated ${title.toLowerCase()}`
              : "No matches"}
          </Text>
        ) : (
          <List hasDividers density="compact">
            {visible.map((c) => (
              <ListItem
                key={c.name}
                label={c.name}
                description={describeCounts(c)}
                endContent={
                  <CheckboxInput
                    label={`Search ${c.name}`}
                    isLabelHidden
                    value={selected.has(c.name)}
                    onChange={(checked) => toggle(c.name, checked)}
                  />
                }
              />
            ))}
          </List>
        )}
      </VStack>
    </VStack>
  );
}

function NewReleasesDialog({
  isOpen,
  onOpenChange,
  days,
  requestReply,
  overrides,
  onSearch,
}: NewReleasesDialogProps) {
  const [candidates, setCandidates] = useState<Candidates>();
  const [error, setError] = useState<string>();
  const [artists, setArtists] = useState<Set<string>>(new Set());
  const [labels, setLabels] = useState<Set<string>>(new Set());

  // Fetch fresh candidates each time the dialog opens, since ratings may have changed. The
  // overrides are only read here, so they're deliberately left out of the deps.
  useEffect(() => {
    if (!isOpen) return;
    setCandidates(undefined);
    setError(undefined);
    requestReply(
      { type: "get-new-releases-candidates" },
      (r: ServerMsg | { type: string }) => {
        if (r.type !== "new-releases-candidates" || !("artists" in r)) {
          setError(
            "message" in r && typeof r.message === "string"
              ? r.message
              : "The artists and labels could not be loaded. Please try again.",
          );
          return;
        }
        setCandidates({ artists: r.artists, labels: r.labels });
        setArtists(initialSelection(r.artists, overrides?.artists));
        setLabels(initialSelection(r.labels, overrides?.labels));
      },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, requestReply]);

  const close = () => onOpenChange(false);

  const nothingSelected = artists.size === 0 && labels.size === 0;

  const submit = () => {
    if (!candidates || nothingSelected) return;
    // Keep the candidates' ranking order so the most relevant names are searched first.
    onSearch(
      candidates.artists.filter((c) => artists.has(c.name)).map((c) => c.name),
      candidates.labels.filter((c) => labels.has(c.name)).map((c) => c.name),
      {
        artists: nextOverrides(candidates.artists, artists, overrides?.artists),
        labels: nextOverrides(candidates.labels, labels, overrides?.labels),
      },
    );
    close();
  };

  const resetToDefaults = () => {
    if (!candidates) return;
    setArtists(initialSelection(candidates.artists));
    setLabels(initialSelection(candidates.labels));
  };

  const content = error ? (
    <Text type="body">{error}</Text>
  ) : !candidates ? (
    <HStack hAlign="center">
      <Spinner label="Loading artists and labels…" />
    </HStack>
  ) : candidates.artists.length === 0 && candidates.labels.length === 0 ? (
    <Text type="body">
      Rate tracks 4★ or higher to see their artists and labels here.
    </Text>
  ) : (
    // Side by side when the dialog is wide enough, stacked otherwise.
    <Grid columns={{ minWidth: 280, max: 2, repeat: "fit" }} gap={4}>
      <NameColumn
        title="Artists"
        candidates={candidates.artists}
        selected={artists}
        setSelected={setArtists}
        onSubmit={submit}
      />
      <NameColumn
        title="Labels"
        candidates={candidates.labels}
        selected={labels}
        setSelected={setLabels}
        onSubmit={submit}
      />
    </Grid>
  );

  return (
    <Dialog
      isOpen={isOpen}
      onOpenChange={onOpenChange}
      purpose="form"
      width={720}
      maxHeight="85vh"
    >
      <Layout
        header={
          <DialogHeader
            title="Latest releases"
            subtitle={`From the last ${days} day${days === 1 ? "" : "s"}`}
            onOpenChange={close}
          />
        }
        content={<LayoutContent>{content}</LayoutContent>}
        footer={
          <LayoutFooter>
            <HStack gap={2} vAlign="center" hAlign="between" wrap="wrap">
              <Button
                label="Reset to defaults"
                variant="ghost"
                isDisabled={!candidates}
                onClick={resetToDefaults}
              />
              <HStack gap={2} vAlign="center" wrap="wrap">
                {candidates && (
                  <Text type="supporting" color="secondary">
                    {artists.size} artist{artists.size === 1 ? "" : "s"},{" "}
                    {labels.size} label{labels.size === 1 ? "" : "s"}
                  </Text>
                )}
                <Button label="Cancel" variant="secondary" onClick={close} />
                <Button
                  label="Search"
                  variant="primary"
                  isDisabled={!candidates || nothingSelected}
                  onClick={submit}
                />
              </HStack>
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default NewReleasesDialog;
