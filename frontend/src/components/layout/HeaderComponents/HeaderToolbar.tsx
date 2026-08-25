import React from 'react';
import {
  Space,
  Avatar,
  Button,
  Dropdown,
  Modal,
  Form,
  Input,
  App,
  type MenuProps,
} from 'antd';
import { useSelector, useDispatch } from 'react-redux';
import { useNavigate } from 'react-router-dom';
import { Moon, Sun, User, LogOut, KeyRound } from 'lucide-react';

import type { RootState } from '@/store';
import { logout } from '@/store/slices/authSlice';
import { loginApi } from '@/features/login/api/login';
import { useTheme } from '../../theme-provider';
import { useHeaderStyle } from '../style';

interface HeaderToolbarProps {
  isMobile: boolean;
}

interface ChangePasswordFormValues {
  old_password: string;
  new_password: string;
}

export const HeaderToolbar: React.FC<HeaderToolbarProps> = ({ isMobile }) => {
  const { theme: antdTheme } = useHeaderStyle();
  const dispatch = useDispatch();
  const navigate = useNavigate();
  const { theme, setTheme } = useTheme();
  const { message } = App.useApp();
  const { userInfo } = useSelector((state: RootState) => state.auth);

  const [changePwdOpen, setChangePwdOpen] = React.useState(false);
  const [changePwdLoading, setChangePwdLoading] = React.useState(false);
  const [pwdForm] = Form.useForm<ChangePasswordFormValues>();

  const openChangePassword = () => {
    pwdForm.resetFields();
    setChangePwdOpen(true);
  };

  const submitChangePassword = async (values: ChangePasswordFormValues) => {
    setChangePwdLoading(true);
    try {
      const resp = await loginApi.changePassword(values);
      if (resp.code === 200) {
        message.success('密码已修改，请重新登录');
        setChangePwdOpen(false);
        dispatch(logout());
        navigate('/login', { replace: true });
      } else {
        message.error(resp.msg || '修改密码失败');
      }
    } catch (error: any) {
      message.error(error?.response?.data?.msg || '修改密码失败');
    } finally {
      setChangePwdLoading(false);
    }
  };

  const userMenuItems: MenuProps['items'] = [
    {
      key: 'change-password',
      icon: <KeyRound size={14} />,
      label: '修改密码',
      style: { fontSize: '1.2rem' },
      onClick: openChangePassword,
    },
    { type: 'divider' },
    {
      key: 'logout',
      icon: <LogOut size={14} />,
      label: '退出登录',
      style: { fontSize: '1.2rem' },
      danger: true,
      onClick: async () => {
        try {
          await loginApi.logout();
        } catch (error) {
          console.error('退出登录失败，请稍后重试', error);
        } finally {
          dispatch(logout());
          navigate('/login', { replace: true });
        }
      },
    },
  ];

  return (
    <Space size='small' align='center'>
      {/* ── 主题切换 ───────────────────────────────────────────── */}
      <Button
        type='text'
        icon={theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />}
        onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
        style={{ color: antdTheme.colorTextBase }}
      />

      {/* ── 用户菜单 ───────────────────────────────────────────── */}
      <Dropdown
        menu={{ items: userMenuItems }}
        align={{ offset: [0, -8] }}
        trigger={['hover']}
        placement='bottomRight'
        arrow
      >
        <Space style={{ cursor: 'pointer', padding: '0 4px' }}>
          <Avatar
            size={isMobile ? 'default' : 'small'}
            style={{ backgroundColor: antdTheme.colorPrimary }}
          >
            {userInfo?.realName?.charAt(0) || <User size={14} />}
          </Avatar>

          {!isMobile && (
            <span
              style={{
                fontSize: 12,
                color: antdTheme.colorTextBase,
                lineHeight: 1,
                display: 'inline-flex',
                alignItems: 'center',
              }}
            >
              {userInfo?.realName || userInfo?.username || '未知用户'}
            </span>
          )}
        </Space>
      </Dropdown>

      {/* ── 修改密码弹窗 ───────────────────────────────────────── */}
      <Modal
        title='修改密码'
        open={changePwdOpen}
        onCancel={() => setChangePwdOpen(false)}
        onOk={() => pwdForm.submit()}
        confirmLoading={changePwdLoading}
        okText='确定'
        cancelText='取消'
      >
        <Form
          form={pwdForm}
          layout='vertical'
          onFinish={submitChangePassword}
        >
          <Form.Item
            name='old_password'
            label='原密码'
            rules={[{ required: true, message: '请输入原密码' }]}
          >
            <Input.Password placeholder='原密码' autoComplete='current-password' />
          </Form.Item>
          <Form.Item
            name='new_password'
            label='新密码'
            rules={[
              { required: true, message: '请输入新密码' },
              { min: 6, max: 100, message: '密码长度 6~100 个字符' },
            ]}
          >
            <Input.Password placeholder='新密码' autoComplete='new-password' />
          </Form.Item>
        </Form>
      </Modal>
    </Space>
  );
};