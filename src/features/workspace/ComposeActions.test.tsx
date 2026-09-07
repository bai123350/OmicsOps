import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ComposeActions } from "./ComposeActions";

describe("ComposeActions", () => {
  it("groups compose actions and closes before invoking available actions", () => {
    const onAttach = vi.fn();
    const onFiles = vi.fn();
    const onReview = vi.fn();
    const onManageSkills = vi.fn();
    const onClose = vi.fn();

    render(<ComposeActions zh={false} onAttach={onAttach} onFiles={onFiles} onReview={onReview} onManageSkills={onManageSkills} onClose={onClose} />);

    expect(screen.getByRole("menu")).toHaveClass("composer-add-menu", "compose-actions");
    expect(screen.getByText("ADD TO MESSAGE")).toBeInTheDocument();
    expect(screen.getByText("SESSION")).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /Attach files/ })).toHaveTextContent("Add images, PDFs, and data files");
    expect(screen.getByRole("menuitem", { name: /Your files/ })).toHaveTextContent("Browse project files");

    fireEvent.click(screen.getByRole("menuitem", { name: /Attach files/ }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onAttach).toHaveBeenCalledTimes(1);
    expect(onClose.mock.invocationCallOrder[0]).toBeLessThan(onAttach.mock.invocationCallOrder[0]);

    fireEvent.click(screen.getByRole("menuitem", { name: /Your files/ }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Request review/ }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Manage skills/ }));
    expect(onFiles).toHaveBeenCalledTimes(1);
    expect(onReview).toHaveBeenCalledTimes(1);
    expect(onManageSkills).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(4);
  });

  it("marks unsupported sharing and skill saving as unavailable and inert", () => {
    const onClose = vi.fn();
    render(<ComposeActions zh={false} onFiles={vi.fn()} onReview={vi.fn()} onClose={onClose} />);

    const share = screen.getByRole("menuitem", { name: /Share as image.*Unavailable/i });
    const save = screen.getByRole("menuitem", { name: /Save as skill.*Unavailable/i });
    expect(share).toBeDisabled();
    expect(save).toBeDisabled();

    fireEvent.click(share);
    fireEvent.click(save);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("disables optional actions when their handlers are not supplied", () => {
    render(<ComposeActions zh={true} onFiles={vi.fn()} onReview={vi.fn()} onClose={vi.fn()} />);

    expect(screen.getByRole("menuitem", { name: /添加文件/ })).toBeDisabled();
    expect(screen.getByRole("menuitem", { name: /管理技能/ })).toBeDisabled();
    expect(screen.getByRole("menuitem", { name: /分享为图片.*暂不可用/ })).toHaveTextContent("分享为图片");
  });
});
