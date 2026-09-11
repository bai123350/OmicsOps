import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { FollowUpQuestions } from "./FollowUpQuestions";

it("suggests three questions and lets the user choose without sending", async () => {
  const choose = vi.fn();
  const generate = vi.fn().mockResolvedValue(["Check evidence?", "Compare methods?", "What next?"]);
  render(<FollowUpQuestions runId="one" generate={generate} onChoose={choose} locale="en-US" />);
  fireEvent.click(await screen.findByRole("button", { name: "Compare methods?" }));
  expect(choose).toHaveBeenCalledWith("Compare methods?");
  expect(generate).toHaveBeenCalledTimes(1);
});
it("discards late questions from the previous run", async () => {
  let resolve!: (value: string[]) => void;
  const generate = vi.fn().mockImplementationOnce(() => new Promise<string[]>((done) => { resolve = done; })).mockResolvedValue(["New one?", "New two?", "New three?"]);
  const { rerender } = render(<FollowUpQuestions runId="one" generate={generate} onChoose={vi.fn()} locale="en-US" />);
  rerender(<FollowUpQuestions runId="two" generate={generate} onChoose={vi.fn()} locale="en-US" />);
  await screen.findByRole("button", { name: "New one?" });
  resolve(["Old one?", "Old two?", "Old three?"]);
  await waitFor(() => expect(screen.queryByText("Old one?")).not.toBeInTheDocument());
});
it("stays hidden when suggestions are disabled or fail", async () => {
  const generate = vi.fn().mockResolvedValue([]);
  const { rerender } = render(<FollowUpQuestions runId="one" generate={generate} onChoose={vi.fn()} locale="en-US" />);
  await waitFor(() => expect(generate).toHaveBeenCalled());
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
  generate.mockRejectedValueOnce(new Error("provider failed"));
  rerender(<FollowUpQuestions runId="two" generate={generate} onChoose={vi.fn()} locale="en-US" />);
  await waitFor(() => expect(generate).toHaveBeenCalledTimes(2));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});
