// 내 회사: 설립, 상장(공모), 배당·유상증자·자사주, 위험도·소개, 해산.
// 조건은 봇이 다시 확인한다. 여기서는 입력을 돕고 조건을 미리 알려 줄 뿐이다.
import { useState, type FormEvent, type ReactNode } from "react";
import { companyAction } from "./api";
import { bpText, ceilTick, floorTick, parseAmount, relativeText, RISK_TEXT, won } from "./format";
import type { Act, CompanyAction, CompanyDetail, StockState } from "./types";

/** 봇의 PAR_VALUE (설립 때 1주의 액면). */
const PAR_VALUE = 5_000;

export function CompanyDesk({
  state,
  now,
  busy,
  act,
  onSelect,
}: {
  state: StockState;
  now: number;
  busy: boolean;
  act: Act;
  onSelect: (code: string) => void;
}) {
  const mine = state.my_companies;
  const canFound = mine.length < state.rules.max_companies;
  const run = (action: CompanyAction) => act((t) => companyAction(t, action));
  return (
    <div className="hts-desk">
      {mine.map((detail) => (
        <ManageCompany key={detail.summary.code} detail={detail} state={state} now={now} busy={busy} run={run} onSelect={onSelect} />
      ))}
      {canFound && <FoundCompany state={state} busy={busy} run={run} />}
      {!canFound && mine.length === 0 && <p className="hts-empty">지금은 회사를 세울 수 없습니다.</p>}
    </div>
  );
}

type Run = (action: CompanyAction) => Promise<unknown>;

function DeskForm({
  title,
  hint,
  children,
  submit,
  disabled,
  busy,
  onSubmit,
  danger,
}: {
  title: string;
  hint?: ReactNode;
  children: ReactNode;
  submit: string;
  disabled?: boolean;
  busy: boolean;
  onSubmit: () => Promise<unknown>;
  danger?: boolean;
}) {
  const handle = (event: FormEvent) => {
    event.preventDefault();
    if (!disabled && !busy) void onSubmit();
  };
  return (
    <form className={`desk-form ${danger ? "danger" : ""}`} onSubmit={handle}>
      <h4>{title}</h4>
      {hint && <p className="desk-hint">{hint}</p>}
      {children}
      <button type="submit" className={danger ? "fold-button" : "gold-button small"} disabled={busy || disabled}>
        {submit}
      </button>
    </form>
  );
}

function Field({ label, children, note }: { label: string; children: ReactNode; note?: ReactNode }) {
  return (
    <label className="desk-field">
      <span>{label}</span>
      {children}
      {note && <small>{note}</small>}
    </label>
  );
}

// ------------------------------------------------------------ 설립

function FoundCompany({ state, busy, run }: { state: StockState; busy: boolean; run: Run }) {
  const rules = state.rules;
  const [name, setName] = useState("");
  const [sector, setSector] = useState(rules.sectors[0]?.key ?? "game");
  const [capitalText, setCapitalText] = useState(() => won(rules.found_min_capital));
  const capital = parseAmount(capitalText);
  const fee = Math.floor((capital * rules.found_fee_bp) / 10_000);
  const cost = capital + fee;
  const shares = Math.max(100, Math.floor(capital / PAR_VALUE));
  const trimmed = name.trim();
  const nameOk = /^[0-9A-Za-z가-힣 ]{2,12}$/.test(trimmed) && !trimmed.includes("  ");
  const problem = !trimmed
    ? ""
    : !nameOk
      ? "회사 이름은 2~12자, 한글·영문·숫자만 쓸 수 있습니다."
      : capital < rules.found_min_capital
        ? `자본금은 ${won(rules.found_min_capital)} 이상이어야 합니다.`
        : cost > state.me.coins
          ? `코인이 부족합니다 (필요 ${won(cost)}, 가진 코인 ${won(state.me.coins)}).`
          : "";
  return (
    <div className="desk-card">
      <h3>회사 설립</h3>
      <p className="desk-hint">
        자본금을 내고 회사를 세우면 발행 주식 {won(shares)}주(액면 {won(PAR_VALUE)})를 모두 갖습니다. 자본금은 회사 현금이 되어 분기마다 사업
        실적(이익·손실)이 쌓이고, 설립 수수료 {bpText(rules.found_fee_bp)}는 복지 금고로 갑니다. 1게임일이 지나면 공모 청약을 열어 상장할 수
        있습니다.
      </p>
      <DeskForm
        title="새 회사"
        submit={`${won(cost)} 내고 설립`}
        disabled={!nameOk || capital < rules.found_min_capital || cost > state.me.coins}
        busy={busy}
        onSubmit={() => run({ action: "found", name: trimmed, sector, capital })}
      >
        <div className="desk-fields">
          <Field label="회사 이름">
            <input value={name} maxLength={12} placeholder="예: 달빛게임즈" onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field label="업종">
            <select value={sector} onChange={(e) => setSector(e.target.value)}>
              {rules.sectors.map((option) => (
                <option key={option.key} value={option.key}>
                  {option.name}
                </option>
              ))}
            </select>
          </Field>
          <Field label="자본금" note={`최소 ${won(rules.found_min_capital)} · 수수료 ${won(fee)}`}>
            <input inputMode="numeric" value={capitalText} onChange={(e) => setCapitalText(e.target.value)} />
          </Field>
        </div>
        {problem && <p className="order-warning">{problem}</p>}
      </DeskForm>
    </div>
  );
}

// ------------------------------------------------------------ 경영

function ManageCompany({
  detail,
  state,
  now,
  busy,
  run,
  onSelect,
}: {
  detail: CompanyDetail;
  state: StockState;
  now: number;
  busy: boolean;
  run: Run;
  onSelect: (code: string) => void;
}) {
  const s = detail.summary;
  const code = s.code;
  const rules = state.rules;
  const position = state.account.positions.find((p) => p.code === code);
  const myShareBp = position ? Math.round((position.qty * 10_000) / Math.max(1, detail.shares)) : 0;
  const isPrivate = s.status === "비상장";
  const listed = s.status === "상장";
  return (
    <div className="desk-card company">
      <header>
        <div>
          <h3>{s.name}</h3>
          <span>
            {code} · {s.sector} · {s.status}
            {s.managed ? " · 관리종목" : ""}
          </span>
        </div>
        <button className="secondary-button small" onClick={() => onSelect(code)}>
          시세 보기
        </button>
      </header>
      <dl className="hts-facts">
        <div>
          <dt>회사 현금 (자본총계)</dt>
          <dd>{won(detail.equity)}</dd>
        </div>
        <div>
          <dt>납입 자본</dt>
          <dd>{won(detail.paid_in)}</dd>
        </div>
        <div>
          <dt>발행 주식</dt>
          <dd>{won(detail.shares)}주</dd>
        </div>
        <div>
          <dt>내 지분</dt>
          <dd>
            {bpText(myShareBp)} <small>({won(position?.qty ?? 0)}주)</small>
          </dd>
        </div>
        <div>
          <dt>주당 순자산</dt>
          <dd>{won(detail.bvps)}</dd>
        </div>
        <div>
          <dt>{listed ? "현재가" : "평가 가격"}</dt>
          <dd>{won(s.price)}</dd>
        </div>
        <div>
          <dt>사업 위험도</dt>
          <dd>{RISK_TEXT[detail.risk] ?? detail.risk}</dd>
        </div>
        <div>
          <dt>다음 실적</dt>
          <dd>{relativeText(detail.next_earnings_at, now)}</dd>
        </div>
      </dl>
      {detail.equity < detail.paid_in / 2 && (
        <p className="order-warning">자본잠식: 회사 현금이 납입 자본의 절반 아래입니다. 계속되면 관리종목·상장폐지로 이어질 수 있습니다.</p>
      )}
      {s.status === "공모 청약" && detail.ipo && (
        <p className="desk-hint">
          공모 청약 진행 중: {won(detail.ipo.shares)}주 중 {won(detail.ipo_requested)}주 청약, {relativeText(detail.ipo.closes_at, now)} 마감.
          청약이 공모 주식의 {bpText(detail.ipo.min_fill_bp)}에 못 미치면 공모가 무산되고 증거금은 돌려줍니다.
        </p>
      )}
      <div className="desk-grid">
        {isPrivate && <IpoForm detail={detail} rules={rules} busy={busy} run={run} />}
        {listed && <DividendForm detail={detail} busy={busy} run={run} />}
        {listed && <RightsForm detail={detail} busy={busy} run={run} />}
        {listed && <BuybackForm detail={detail} busy={busy} run={run} />}
        {(isPrivate || listed) && <RiskForm detail={detail} busy={busy} run={run} />}
        {(isPrivate || listed) && <DescribeForm detail={detail} busy={busy} run={run} />}
        {(isPrivate || listed) && <DissolveForm detail={detail} listed={listed} busy={busy} run={run} />}
      </div>
    </div>
  );
}

function IpoForm({ detail, rules, busy, run }: { detail: CompanyDetail; rules: StockState["rules"]; busy: boolean; run: Run }) {
  const low = ceilTick(Math.ceil(detail.bvps * 0.8));
  const high = floorTick(Math.floor(detail.bvps * 3));
  const minShares = Math.max(1, Math.floor(detail.shares / 10));
  const [priceText, setPriceText] = useState(() => won(floorTick(Math.max(low, detail.bvps))));
  const [sharesText, setSharesText] = useState(() => won(Math.max(minShares, Math.floor(detail.shares / 4))));
  const price = parseAmount(priceText);
  const shares = parseAmount(sharesText);
  const raise = price * shares;
  const fee = Math.floor((raise * rules.ipo_fee_bp) / 10_000);
  const tooSmall = detail.equity < rules.listing_min_equity;
  return (
    <DeskForm
      title="상장 (공모 청약 열기)"
      hint={
        <>
          공모가는 주당 순자산의 0.8~3배({won(low)}~{won(high)}), 공모 주식은 {won(minShares)}~{won(detail.shares)}주입니다. 청약은 1게임일 동안
          받고, 모인 대금에서 수수료 {bpText(rules.ipo_fee_bp)}를 뺀 만큼이 회사 현금이 됩니다. 상장하면 대표 지분은 {rules.lockup_days}게임일 동안
          팔 수 없습니다.
          {tooSmall && ` 자본총계가 ${won(rules.listing_min_equity)} 이상이어야 상장할 수 있습니다.`}
        </>
      }
      submit={raise > 0 ? `공모 열기 (최대 ${won(raise - fee)} 조달)` : "공모 열기"}
      disabled={tooSmall || price <= 0 || shares <= 0}
      busy={busy}
      onSubmit={() => run({ action: "ipo", code: detail.summary.code, price, shares })}
    >
      <div className="desk-fields">
        <Field label="공모가">
          <input inputMode="numeric" value={priceText} onChange={(e) => setPriceText(e.target.value)} />
        </Field>
        <Field label="새로 발행할 주식">
          <input inputMode="numeric" value={sharesText} onChange={(e) => setSharesText(e.target.value)} />
        </Field>
      </div>
    </DeskForm>
  );
}

function DividendForm({ detail, busy, run }: { detail: CompanyDetail; busy: boolean; run: Run }) {
  const [text, setText] = useState("");
  const perShare = parseAmount(text);
  const total = perShare * detail.shares;
  const pending = detail.pending_dividend !== null;
  return (
    <DeskForm
      title="현금 배당"
      hint={
        pending
          ? `주당 ${won(detail.pending_dividend!.per_share)} 배당이 지급을 기다리고 있습니다.`
          : `회사 현금에서 주주에게 보유 주식 수만큼 나눠 줍니다. 다음 게임일이 시작될 때 지급됩니다. (최대 주당 ${won(Math.floor(detail.equity / Math.max(1, detail.shares)))})`
      }
      submit={total > 0 ? `총 ${won(total)} 배당` : "배당 결정"}
      disabled={pending || perShare <= 0 || total > detail.equity}
      busy={busy}
      onSubmit={async () => {
        const done = await run({ action: "dividend", code: detail.summary.code, per_share: perShare });
        if (done) setText("");
      }}
    >
      <Field label="주당 배당금">
        <input inputMode="numeric" placeholder="0" value={text} onChange={(e) => setText(e.target.value)} />
      </Field>
    </DeskForm>
  );
}

function RightsForm({ detail, busy, run }: { detail: CompanyDetail; busy: boolean; run: Run }) {
  const price = detail.summary.price;
  const low = ceilTick(Math.floor((price * 7) / 10));
  const [sharesText, setSharesText] = useState("");
  const [priceText, setPriceText] = useState(() => won(floorTick(Math.floor(price * 0.85))));
  const shares = parseAmount(sharesText);
  const issue = parseAmount(priceText);
  const running = detail.rights !== null;
  return (
    <DeskForm
      title="유상증자 (주주배정)"
      hint={
        running
          ? "진행 중인 유상증자가 있습니다."
          : `지금 주주에게 보유 비율대로 신주인수권을 주고 1게임일 동안 청약을 받습니다. 발행가는 현재가의 70~100%(${won(low)}~${won(price)}), 들어온 대금은 회사 현금이 됩니다.`
      }
      submit={shares > 0 && issue > 0 ? `신주 ${won(shares)}주 발행 (최대 ${won(shares * issue)})` : "유상증자 결정"}
      disabled={running || shares <= 0 || issue <= 0}
      busy={busy}
      onSubmit={async () => {
        const done = await run({ action: "rights", code: detail.summary.code, shares, price: issue });
        if (done) setSharesText("");
      }}
    >
      <div className="desk-fields">
        <Field label="신주 수">
          <input inputMode="numeric" placeholder="0" value={sharesText} onChange={(e) => setSharesText(e.target.value)} />
        </Field>
        <Field label="발행가">
          <input inputMode="numeric" value={priceText} onChange={(e) => setPriceText(e.target.value)} />
        </Field>
      </div>
    </DeskForm>
  );
}

function BuybackForm({ detail, busy, run }: { detail: CompanyDetail; busy: boolean; run: Run }) {
  const [text, setText] = useState("");
  const budget = parseAmount(text);
  const max = Math.floor(detail.equity / 2);
  const running = detail.buyback !== null;
  return (
    <DeskForm
      title="자사주 매입·소각"
      hint={
        running
          ? "진행 중인 자사주 매입이 있습니다."
          : `회사 현금으로 1게임일 동안 시장에서 주식을 사서 없앱니다 (예산은 회사 현금의 절반 ${won(max)}까지). 그동안 대표는 주식을 팔 수 없습니다.`
      }
      submit={budget > 0 ? `${won(budget)} 매입` : "자사주 매입"}
      disabled={running || budget <= 0 || budget > max}
      busy={busy}
      onSubmit={async () => {
        const done = await run({ action: "buyback", code: detail.summary.code, budget });
        if (done) setText("");
      }}
    >
      <Field label="매입 예산">
        <input inputMode="numeric" placeholder="0" value={text} onChange={(e) => setText(e.target.value)} />
      </Field>
    </DeskForm>
  );
}

function RiskForm({ detail, busy, run }: { detail: CompanyDetail; busy: boolean; run: Run }) {
  const [risk, setRisk] = useState(detail.risk);
  return (
    <DeskForm
      title="사업 위험도"
      hint="높을수록 분기 실적(이익·손실)의 폭이 커지고 주가도 크게 움직입니다. 실제 1주에 한 번 바꿀 수 있습니다."
      submit="위험도 바꾸기"
      disabled={risk === detail.risk}
      busy={busy}
      onSubmit={() => run({ action: "risk", code: detail.summary.code, risk })}
    >
      <Field label="위험도">
        <select value={risk} onChange={(e) => setRisk(Number(e.target.value))}>
          {[1, 2, 3, 4, 5].map((level) => (
            <option key={level} value={level}>
              {RISK_TEXT[level]}
            </option>
          ))}
        </select>
      </Field>
    </DeskForm>
  );
}

function DescribeForm({ detail, busy, run }: { detail: CompanyDetail; busy: boolean; run: Run }) {
  const [text, setText] = useState(detail.description);
  return (
    <DeskForm
      title="회사 소개"
      hint="종목 정보에 보이는 소개 문구입니다 (120자까지)."
      submit="소개 저장"
      disabled={text.trim() === detail.description || [...text.trim()].length > 120}
      busy={busy}
      onSubmit={() => run({ action: "describe", code: detail.summary.code, text: text.trim() })}
    >
      <Field label="소개" note={`${[...text.trim()].length}/120`}>
        <textarea rows={3} value={text} onChange={(e) => setText(e.target.value)} />
      </Field>
    </DeskForm>
  );
}

function DissolveForm({ detail, listed, busy, run }: { detail: CompanyDetail; listed: boolean; busy: boolean; run: Run }) {
  const [confirm, setConfirm] = useState("");
  const name = detail.summary.name;
  return (
    <DeskForm
      danger
      title="해산"
      hint={
        listed
          ? "대표 지분이 50% 이상이어야 합니다. 1게임일 거래정지 뒤 남은 회사 현금을 주주에게 주식 수만큼 나누고 상장폐지합니다. 되돌릴 수 없습니다."
          : "비상장 회사는 바로 해산되고, 남은 회사 현금을 돌려받습니다. 되돌릴 수 없습니다."
      }
      submit="해산 결정"
      disabled={confirm.trim() !== name}
      busy={busy}
      onSubmit={() => run({ action: "dissolve", code: detail.summary.code, confirm: confirm.trim() })}
    >
      <Field label={`확인: 회사 이름 '${name}'을(를) 그대로 입력`}>
        <input value={confirm} onChange={(e) => setConfirm(e.target.value)} />
      </Field>
    </DeskForm>
  );
}
