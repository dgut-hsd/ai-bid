import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
  container: css`
    display: flex;
    flex-direction: column;
    height: 100%;
    border-radius: 12px;
    overflow: hidden;
    box-shadow: 0 1px 3px 0 rgb(0 0 0 / 0.04), 0 1px 2px -1px rgb(0 0 0 / 0.04);
    background: ${token.colorBgContainer};
    border: 1px solid ${token.colorBorderSecondary};
  `,

  /* ── Header ── */
  header: css`
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    border-bottom: 1px solid ${token.colorBorderSecondary};
    background: ${token.colorBgContainer};
    flex-shrink: 0;
  `,

  headerInfo: css`
    display: flex;
    align-items: center;
    gap: 10px;
  `,

  headerAvatar: css`
    width: 36px;
    height: 36px;
    border-radius: 10px;
    background: linear-gradient(
      135deg,
      ${token.colorPrimary} 0%,
      ${token.colorPrimaryActive} 100%
    );
    display: flex;
    align-items: center;
    justify-content: center;
    color: #fff;
    font-size: 17px;
    flex-shrink: 0;
  `,

  headerText: css`
    display: flex;
    flex-direction: column;
  `,

  headerTitle: css`
    font-size: 15px;
    font-weight: 600;
    color: ${token.colorTextBase};
    line-height: 1.3;
  `,

  headerStatus: css`
    font-size: 12px;
    color: ${token.colorTextTertiary};
    display: flex;
    align-items: center;
    gap: 5px;
  `,

  statusDot: css`
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #22c55e;
    box-shadow: 0 0 0 2px rgb(34 197 94 / 0.2);
  `,

  /* ── Message list ── */
  messageList: css`
    flex: 1;
    overflow-y: auto;
    padding: 14px 12px;
    background: ${token.colorBgLayout};

    &::-webkit-scrollbar {
      width: 4px;
    }
    &::-webkit-scrollbar-thumb {
      background: ${token.colorBorderSecondary};
      border-radius: 2px;
    }
  `,

  /* ── Empty state ── */
  centered: css`
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: 20px;
    gap: 8px;
  `,

  emptyIcon: css`
    width: 48px;
    height: 48px;
    border-radius: 12px;
    background: linear-gradient(
      135deg,
      ${token.colorPrimary}15,
      ${token.colorPrimary}08
    );
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 24px;
    color: ${token.colorPrimary};
    margin-bottom: 4px;
  `,

  /* ── Typing indicator ── */
  typing: css`
    display: flex;
    align-items: flex-end;
    padding: 4px 0;
    gap: 8px;
  `,

  typingAvatar: css`
    width: 28px;
    height: 28px;
    border-radius: 8px;
    background: linear-gradient(
      135deg,
      ${token.colorPrimary}99,
      ${token.colorPrimaryActive}99
    );
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    color: #fff;
    flex-shrink: 0;
  `,

  typingBubble: css`
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 10px 14px;
    background: ${token.colorBgContainer};
    border: 1px solid ${token.colorBorderSecondary};
    border-radius: 16px 16px 16px 4px;

    span {
      width: 6px;
      height: 6px;
      border-radius: 50%;
      background: ${token.colorTextQuaternary};
      animation: typingBounce 1.4s ease-in-out infinite;

      &:nth-child(2) {
        animation-delay: 0.2s;
      }
      &:nth-child(3) {
        animation-delay: 0.4s;
      }
    }

    @keyframes typingBounce {
      0%,
      60%,
      100% {
        transform: translateY(0);
        opacity: 0.3;
      }
      30% {
        transform: translateY(-4px);
        opacity: 1;
      }
    }
  `,

  /* ── Footer / Input ── */
  footer: css`
    padding: 10px 14px 12px;
    border-top: 1px solid ${token.colorBorderSecondary};
    background: ${token.colorBgContainer};
    flex-shrink: 0;
  `,

  inputRow: css`
    display: flex;
    gap: 8px;
    align-items: flex-end;
  `,

  textArea: css`
    flex: 1;
    border-radius: 12px !important;
    padding: 10px 14px !important;
    font-size: 14px;
    line-height: 1.5;
    resize: none;
    border: 1px solid ${token.colorBorderSecondary} !important;
    background: ${token.colorBgLayout} !important;
    transition: border-color 0.2s, box-shadow 0.2s, background 0.2s;

    &:focus {
      border-color: ${token.colorPrimary} !important;
      box-shadow: 0 0 0 2px ${token.colorPrimary}20 !important;
      background: ${token.colorBgContainer} !important;
    }

    &::placeholder {
      color: ${token.colorTextQuaternary};
    }
  `,


      /* ── Markdown content ── */
      markdownContent: css`
        font-size: 13px;
        line-height: 1.7;
        color: ${token.colorTextBase};
        word-break: break-word;

        h1, h2, h3, h4, h5, h6 {
          margin: 14px 0 6px;
          line-height: 1.4;
          font-weight: 600;
          color: ${token.colorTextBase};

          &:first-child {
            margin-top: 0;
          }
        }

        h1 { font-size: 18px; }
        h2 { font-size: 16px; }
        h3 { font-size: 15px; }
        h4 { font-size: 14px; }
        h5 { font-size: 13px; }
        h6 {
          font-size: 12px;
          color: ${token.colorTextSecondary};
        }

        p {
          margin: 0 0 8px;
          &:last-child {
            margin-bottom: 0;
          }
        }

        ul, ol {
          margin: 4px 0 8px;
          padding-left: 20px;
        }

        li {
          margin-bottom: 2px;
        }

        blockquote {
          margin: 8px 0;
          padding: 6px 12px;
          border-left: 3px solid ${token.colorBorderSecondary};
          background: ${token.colorFillAlter};
          border-radius: 0 6px 6px 0;
          color: ${token.colorTextSecondary};

          p {
            margin: 0;
          }
        }

        strong {
          font-weight: 600;
        }

        a {
          color: ${token.colorPrimary};
          text-decoration: none;

          &:hover {
            text-decoration: underline;
          }
        }

        code {
          padding: 1px 5px;
          border-radius: 4px;
          background: ${token.colorFillAlter};
          font-size: 12px;
          font-family: 'SF Mono', 'Monaco', 'Menlo', monospace;
        }

        pre {
          margin: 8px 0;
          padding: 10px 12px;
          border-radius: 8px;
          background: ${token.colorFillAlter};
          overflow-x: auto;
          font-size: 12px;
          line-height: 1.5;

          code {
            padding: 0;
            background: none;
          }
        }

        hr {
          margin: 12px 0;
          border: none;
          border-top: 1px solid ${token.colorBorderSecondary};
        }

        table {
          margin: 8px 0;
          border-collapse: collapse;
          display: block;
          width: 100%;
          max-width: 100%;
          overflow-x: auto;
          font-size: 12px;

          th, td {
            border: 1px solid ${token.colorBorderSecondary};
            padding: 6px 10px;
            text-align: left;
            white-space: nowrap;
          }

          th {
            background: ${token.colorFillAlter};
            font-weight: 600;
          }
        }
      `,
  sendBtn: css`
    width: 38px;
    height: 38px;
    border-radius: 10px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    background: ${token.colorPrimary};
    border: none;
    color: #fff;
    cursor: pointer;
    transition: background 0.2s, transform 0.15s;

    &:hover:not(:disabled) {
      background: ${token.colorPrimaryHover} !important;
      transform: scale(1.04);
    }

    &:disabled {
      background: ${token.colorBorderSecondary} !important;
      color: ${token.colorTextQuaternary} !important;
      cursor: not-allowed;
      transform: none;
    }
  `,
}));
