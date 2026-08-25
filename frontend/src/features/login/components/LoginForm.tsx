import { Form, Input, Checkbox, Button } from 'antd';
import type { FormInstance } from 'antd';
import { UserOutlined, LockOutlined } from '@ant-design/icons';
import { useStyles } from '../style';

export type LoginFormValues = {
   username: string;
   password: string;
   remember?: boolean;
};

interface LoginFormProps {
   form: FormInstance<LoginFormValues>;
   loading: boolean;
   onFinish: (values: LoginFormValues) => void;
   buttonClass: string;
}

export function LoginForm({
   form,
   loading,
   onFinish,
   buttonClass,
}: LoginFormProps) {
   const { styles, theme } = useStyles();

   return (
      <Form
         form={form}
         name='login'
         onFinish={onFinish}
         layout='vertical'
         size='large'
         autoComplete='off'
         initialValues={{ remember: false }}
         validateTrigger={['onBlur', 'onSubmit']}
      >
         <Form.Item
            name='username'
            label={<span>账号</span>}
            rules={[{ required: true, message: '请输入账号' }]}
         >
            <Input
               prefix={
                  <UserOutlined style={{ color: theme.colorTextDescription }} />
               }
               placeholder='请输入账号（用户名）'
               autoComplete='username'
            />
         </Form.Item>

         <Form.Item
            name='password'
            label={<span>密码</span>}
            rules={[{ required: true, message: '请输入密码' }]}
         >
            <Input.Password
               prefix={
                  <LockOutlined style={{ color: theme.colorTextDescription }} />
               }
               placeholder='请输入密码'
               autoComplete='current-password'
            />
         </Form.Item>

         <Form.Item>
            <div className={styles.loginActionRow}>
               <Checkbox name='remember'>记住我</Checkbox>
            </div>
         </Form.Item>

         <Form.Item>
            <Button
               type='primary'
               htmlType='submit'
               loading={loading}
               className={buttonClass}
            >
               {loading ? '登录中...' : '登录'}
            </Button>
         </Form.Item>
      </Form>
   );
}