import { useEffect, useState } from "react";

export function FollowUpQuestions({ runId, generate, onChoose, locale }: {
  runId: string; generate: (runId: string) => Promise<string[]>; onChoose: (question: string) => void; locale: string;
}) {
  const [result, setResult] = useState<{ runId: string; questions: string[] } | null>(null);
  useEffect(() => {
    let active = true;
    generate(runId).then((questions) => {
      if (active) setResult({ runId, questions: Array.isArray(questions) ? questions : [] });
    }).catch(() => { if (active) setResult({ runId, questions: [] }); });
    return () => { active = false; };
  }, [runId, generate]);
  if (result?.runId !== runId || result.questions.length !== 3) return null;
  return <section className="session-follow-ups" aria-label={locale === "zh-CN" ? "后续问题建议" : "Suggested follow-up questions"}>
    {result.questions.map((question) => <button key={question} type="button" onClick={() => onChoose(question)}>{question}</button>)}
  </section>;
}
